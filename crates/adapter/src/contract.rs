use bm_sdk::{
    AgentToolHint, MemoryCapabilityCatalog, MemoryCloseReport, MemoryCloseRequest,
    MemoryExportReport, MemoryExportRequest, MemoryGovernancePolicyMutationReport,
    MemoryImportReport, MemoryImportRequest, MemoryInspectionReport, MemoryInspectionRequest,
    MemoryLongTermDetailReport, MemoryLongTermDetailRequest, MemoryLongTermListReport,
    MemoryLongTermListRequest, MemoryLongTermMutationReport, MemoryLongTermMutationRequest,
    MemoryLongTermPolicyRequest, MemoryMaintenanceReport, MemoryMaintenanceRequest,
    MemoryProjectionReport, MemoryProjectionRequest, MemoryRecallReport, MemoryRecallRequest,
    MemoryRecoverReport, MemoryRecoverRequest, MemoryReplayReport, MemoryReplayRequest,
    MemoryTranscriptAttrWriteReport, MemoryTranscriptAttrWriteRequest, MemoryWriteReport,
    MemoryWriteRequest,
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
    TranscriptAttrWrite,
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
    LongTermMutate(Box<MemoryLongTermMutationRequest>),
    LongTermPolicy(MemoryLongTermPolicyRequest),
    TranscriptAttrWrite(MemoryTranscriptAttrWriteRequest),
    Capabilities,
    Close(MemoryCloseRequest),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterProjectionAuditSummary {
    pub projection_id: String,
    pub source_budget_chars: usize,
    pub render_budget_chars: usize,
    pub injected: bool,
    pub truncated: bool,
    pub runtime_private_context_allowed: bool,
    pub foreground_disclosure_allowed: bool,
    pub private_gate_reason: String,
    pub evidence_ref_count: usize,
    pub budget_decision_count: usize,
    pub privacy_decision_count: usize,
    pub dropped_candidate_count: usize,
    pub faithfulness_passed: bool,
    pub unsupported_claim_count: usize,
    pub disclosure_integrity_passed: bool,
    pub raw_private_violation_count: u32,
    pub agent_tool_rejection_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterProjectionReport {
    pub projection_block: String,
    pub chars: usize,
    pub agent_tool_hints: Vec<AgentToolHint>,
    pub audit: AdapterProjectionAuditSummary,
}

impl From<MemoryProjectionReport> for AdapterProjectionReport {
    fn from(report: MemoryProjectionReport) -> Self {
        let audit = AdapterProjectionAuditSummary {
            projection_id: report.audit.projection_id.clone(),
            source_budget_chars: report.audit.source_budget_chars,
            render_budget_chars: report.audit.render_budget_chars,
            injected: report.audit.injected,
            truncated: report.audit.truncated,
            runtime_private_context_allowed: report
                .audit
                .private_gate
                .runtime_private_context_allowed,
            foreground_disclosure_allowed: report.audit.private_gate.foreground_disclosure_allowed,
            private_gate_reason: report.audit.private_gate.reason.clone(),
            evidence_ref_count: report.subject_projection.evidence_refs.len(),
            budget_decision_count: report.subject_projection.budget_decisions.len(),
            privacy_decision_count: report.subject_projection.privacy_decisions.len(),
            dropped_candidate_count: report.subject_projection.dropped_candidates.len(),
            faithfulness_passed: report.projection_faithfulness.passed,
            unsupported_claim_count: report.projection_faithfulness.unsupported_claims.len(),
            disclosure_integrity_passed: report.private_disclosure_integrity.passed,
            raw_private_violation_count: report
                .private_disclosure_integrity
                .raw_private_violation_count,
            agent_tool_rejection_count: report.audit.agent_tools.rejected.len(),
        };
        let projection_block = report.projection_surfaces.ui_api;
        Self {
            chars: projection_block.chars().count(),
            projection_block,
            agent_tool_hints: report.runtime_projection.agent_tool_hints,
            audit,
        }
    }
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
            Self::TranscriptAttrWrite(_) => AdapterOperation::TranscriptAttrWrite,
            Self::Capabilities => AdapterOperation::Capabilities,
            Self::Close(_) => AdapterOperation::Close,
        }
    }
}

pub enum AdapterSdkReport {
    Write(Box<MemoryWriteReport>),
    Recall(Box<MemoryRecallReport>),
    Project(Box<AdapterProjectionReport>),
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
    TranscriptAttrWrite(Box<MemoryTranscriptAttrWriteReport>),
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
            Self::TranscriptAttrWrite(_) => "TranscriptAttrWrite",
            Self::Capabilities(_) => "Capabilities",
            Self::Close(_) => "Close",
        };
        f.write_str(name)
    }
}
