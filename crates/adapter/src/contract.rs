use bm_sdk::{
    AgentToolHint, GovernedRecallPublicReportV1, MemoryCapabilityCatalog, MemoryCloseReport,
    MemoryCloseRequest, MemoryGovernancePolicyMutationReport, MemoryInspectionReport,
    MemoryInspectionRequest, MemoryLongTermDetailReport, MemoryLongTermDetailRequest,
    MemoryLongTermListReport, MemoryLongTermListRequest, MemoryLongTermMutationReport,
    MemoryLongTermMutationRequest, MemoryLongTermPolicyRequest, MemoryMaintenanceReport,
    MemoryMaintenanceRequest, MemoryProjectionReport, MemoryProjectionRequest, MemoryRecallReport,
    MemoryRecallRequest, MemoryRecallTemporalOperation, MemoryRecoverReport, MemoryRecoverRequest,
    MemoryReplayReport, MemoryReplayRequest, MemoryTranscriptAttrWriteReport,
    MemoryTranscriptAttrWriteRequest, MemoryWriteReport, MemoryWriteRequest,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicUsize, Ordering};

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

static NEXT_REQUEST_IDENTITY_OWNER: AtomicUsize = AtomicUsize::new(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdapterRequestIdentity {
    pub request_id: String,
    pub audit_id: String,
    pub idempotency_key: String,
}

#[derive(Debug)]
pub struct AdapterRequestIdentityOwner {
    transport: TransportKind,
    source_id: String,
    principal: String,
    owner_sequence: usize,
    next_request_sequence: AtomicUsize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdapterRequestIdentityError {
    EmptySourceId,
    EmptyPrincipal,
    EmptyExplicitIdempotencyKey,
    SequenceExhausted,
}

impl std::fmt::Display for AdapterRequestIdentityError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::EmptySourceId => "transport request identity source_id must not be empty",
            Self::EmptyPrincipal => {
                "transport request identity authenticated principal must not be empty"
            }
            Self::EmptyExplicitIdempotencyKey => {
                "explicit transport idempotency key must not be empty"
            }
            Self::SequenceExhausted => "transport request identity sequence exhausted",
        })
    }
}

impl std::error::Error for AdapterRequestIdentityError {}

impl AdapterRequestIdentityOwner {
    pub fn new(
        transport: TransportKind,
        source_id: impl Into<String>,
        authenticated_principal: impl Into<String>,
    ) -> Self {
        Self {
            transport,
            source_id: source_id.into(),
            principal: authenticated_principal.into(),
            owner_sequence: next_sequence(&NEXT_REQUEST_IDENTITY_OWNER).unwrap_or(usize::MAX),
            next_request_sequence: AtomicUsize::new(1),
        }
    }

    pub fn principal(&self) -> &str {
        self.principal.trim()
    }

    pub fn issue(
        &self,
        explicit_idempotency_key: Option<&str>,
    ) -> Result<AdapterRequestIdentity, AdapterRequestIdentityError> {
        let source_id = self.source_id.trim();
        if source_id.is_empty() {
            return Err(AdapterRequestIdentityError::EmptySourceId);
        }
        let principal = self.principal();
        if principal.is_empty() {
            return Err(AdapterRequestIdentityError::EmptyPrincipal);
        }
        if self.owner_sequence == usize::MAX {
            return Err(AdapterRequestIdentityError::SequenceExhausted);
        }
        let request_sequence = next_sequence(&self.next_request_sequence)?;
        let transport = transport_identity_label(self.transport);
        let request_id = format!(
            "{transport}-request-{}-{request_sequence}",
            self.owner_sequence
        );
        let idempotency_key = match explicit_idempotency_key {
            Some(key) => {
                let key = key.trim();
                if key.is_empty() {
                    return Err(AdapterRequestIdentityError::EmptyExplicitIdempotencyKey);
                }
                derived_idempotency_key("explicit", transport, &[principal, key])
            }
            None => derived_idempotency_key(
                "automatic",
                transport,
                &[
                    principal,
                    source_id,
                    &self.owner_sequence.to_string(),
                    &request_sequence.to_string(),
                ],
            ),
        };
        Ok(AdapterRequestIdentity {
            audit_id: format!("audit-{request_id}"),
            request_id,
            idempotency_key,
        })
    }
}

fn derived_idempotency_key(mode: &str, transport: &str, fields: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for field in std::iter::once("bm_adapter_request_identity_v1")
        .chain(std::iter::once(mode))
        .chain(std::iter::once(transport))
        .chain(fields.iter().copied())
    {
        hasher.update((field.len() as u64).to_be_bytes());
        hasher.update(field.as_bytes());
    }
    format!("{mode}:v1:sha256:{:x}", hasher.finalize())
}

fn next_sequence(counter: &AtomicUsize) -> Result<usize, AdapterRequestIdentityError> {
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(1)
        })
        .map_err(|_| AdapterRequestIdentityError::SequenceExhausted)
}

const fn transport_identity_label(transport: TransportKind) -> &'static str {
    match transport {
        TransportKind::Sdk => "sdk",
        TransportKind::Cli => "cli",
        TransportKind::Http => "http",
        TransportKind::Wss => "wss",
        TransportKind::Mcp => "mcp",
        TransportKind::A2a => "a2a",
    }
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
    LongTermList,
    LongTermDetail,
    LongTermMutate,
    LongTermPolicy,
    TranscriptAttrWrite,
    Capabilities,
    Subscribe,
    Close,
}

impl AdapterOperation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::Recall => "recall",
            Self::Project => "project",
            Self::Maintain => "maintain",
            Self::Inspect => "inspect",
            Self::Recover => "recover",
            Self::Replay => "replay",
            Self::LongTermList => "long_term_list",
            Self::LongTermDetail => "long_term_detail",
            Self::LongTermMutate => "long_term_mutate",
            Self::LongTermPolicy => "long_term_policy",
            Self::TranscriptAttrWrite => "transcript_attr_write",
            Self::Capabilities => "capabilities",
            Self::Subscribe => "subscribe",
            Self::Close => "close",
        }
    }

    pub const fn requires_idempotency(self) -> bool {
        match self {
            Self::Write
            | Self::Maintain
            | Self::Recover
            | Self::LongTermMutate
            | Self::LongTermPolicy
            | Self::TranscriptAttrWrite
            | Self::Close => true,
            Self::Recall
            | Self::Project
            | Self::Inspect
            | Self::Replay
            | Self::LongTermList
            | Self::LongTermDetail
            | Self::Capabilities
            | Self::Subscribe => false,
        }
    }
}

impl std::fmt::Display for AdapterOperation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
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
    pub governed_recall: GovernedRecallPublicReportV1,
}

impl From<MemoryProjectionReport> for AdapterProjectionReport {
    fn from(report: MemoryProjectionReport) -> Self {
        let safe_audit = report.audit();
        let audit = AdapterProjectionAuditSummary {
            projection_id: safe_audit.projection_id.clone(),
            source_budget_chars: safe_audit.source_budget_chars,
            render_budget_chars: safe_audit.render_budget_chars,
            injected: safe_audit.injected,
            truncated: safe_audit.truncated,
            runtime_private_context_allowed: safe_audit.runtime_private_context_allowed,
            foreground_disclosure_allowed: safe_audit.foreground_disclosure_allowed,
            private_gate_reason: safe_audit.private_gate_reason.clone(),
            evidence_ref_count: safe_audit.evidence_ref_count,
            budget_decision_count: safe_audit.budget_decision_count,
            privacy_decision_count: safe_audit.privacy_decision_count,
            dropped_candidate_count: safe_audit.dropped_candidate_count,
            faithfulness_passed: safe_audit.faithfulness_passed,
            unsupported_claim_count: safe_audit.unsupported_claim_count,
            disclosure_integrity_passed: safe_audit.disclosure_integrity_passed,
            raw_private_violation_count: safe_audit.raw_private_violation_count,
            agent_tool_rejection_count: safe_audit.agent_tool_rejection_count,
        };
        Self {
            chars: report.ui_api_chars(),
            projection_block: report.ui_api_projection().to_string(),
            agent_tool_hints: report.agent_tool_hints().to_vec(),
            audit,
            governed_recall: report.governed_public_report().clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterGovernedRecallSafeReportV1 {
    pub query: String,
    pub temporal_operation: MemoryRecallTemporalOperation,
    pub governed_recall: GovernedRecallPublicReportV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterGovernedProjectSafeReportV1 {
    pub temporal_operation: MemoryRecallTemporalOperation,
    pub projection_block: String,
    pub chars: usize,
    pub agent_tool_hints: Vec<AgentToolHint>,
    pub audit: AdapterProjectionAuditSummary,
    pub governed_recall: GovernedRecallPublicReportV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "operation",
    content = "report",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum AdapterGovernedSafeReportV1 {
    Recall(AdapterGovernedRecallSafeReportV1),
    Project(AdapterGovernedProjectSafeReportV1),
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
            Self::LongTermList(_) => AdapterOperation::LongTermList,
            Self::LongTermDetail(_) => AdapterOperation::LongTermDetail,
            Self::LongTermMutate(_) => AdapterOperation::LongTermMutate,
            Self::LongTermPolicy(_) => AdapterOperation::LongTermPolicy,
            Self::TranscriptAttrWrite(_) => AdapterOperation::TranscriptAttrWrite,
            Self::Capabilities => AdapterOperation::Capabilities,
            Self::Close(_) => AdapterOperation::Close,
        }
    }

    pub fn pin_accepted_at(&mut self, accepted_at: u64) {
        if let Self::Write(MemoryWriteRequest::Procedural { writes, .. }) = self {
            for write in writes {
                write.write.observed_at = accepted_at;
            }
        }
    }

    /// Produces stable typed material for transport-independent idempotency hashing.
    pub fn idempotency_fingerprint_material(&self) -> Result<Vec<u8>, serde_json::Error> {
        let payload = match self {
            Self::Write(request) => {
                let mut request = request.clone();
                if let MemoryWriteRequest::Procedural { writes, .. } = &mut request {
                    for write in writes {
                        write.write.observed_at = 0;
                    }
                }
                serde_json::to_vec(&request)?
            }
            Self::Recall(_) | Self::Project(_) => Vec::new(),
            Self::Maintain(request) => serde_json::to_vec(request)?,
            Self::Inspect(_) => Vec::new(),
            Self::Recover(request) => serde_json::to_vec(request)?,
            Self::Replay(_) => Vec::new(),
            Self::LongTermList(request) => serde_json::to_vec(request)?,
            Self::LongTermDetail(request) => serde_json::to_vec(request)?,
            Self::LongTermMutate(request) => serde_json::to_vec(request)?,
            Self::LongTermPolicy(request) => serde_json::to_vec(request)?,
            Self::TranscriptAttrWrite(request) => serde_json::to_vec(request)?,
            Self::Capabilities => Vec::new(),
            Self::Close(request) => serde_json::to_vec(request)?,
        };
        let operation = self.operation().as_str().as_bytes();
        const DOMAIN: &[u8] = b"bm/adapter-operation-idempotency/v1\0";
        let mut material = Vec::with_capacity(DOMAIN.len() + 16 + operation.len() + payload.len());
        material.extend_from_slice(DOMAIN);
        material.extend_from_slice(&(operation.len() as u64).to_be_bytes());
        material.extend_from_slice(operation);
        material.extend_from_slice(&(payload.len() as u64).to_be_bytes());
        material.extend_from_slice(&payload);
        Ok(material)
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
    LongTermList(Box<MemoryLongTermListReport>),
    LongTermDetail(Box<MemoryLongTermDetailReport>),
    LongTermMutate(Box<MemoryLongTermMutationReport>),
    LongTermPolicy(Box<MemoryGovernancePolicyMutationReport>),
    TranscriptAttrWrite(Box<MemoryTranscriptAttrWriteReport>),
    Capabilities(Box<MemoryCapabilityCatalog>),
    Close(Box<MemoryCloseReport>),
}

impl AdapterSdkReport {
    pub const fn public_kind(&self) -> &'static str {
        match self {
            Self::Write(_) => "write",
            Self::Recall(_) => "recall",
            Self::Project(_) => "project",
            Self::Maintain(_) => "maintain",
            Self::Inspect(_) => "inspect",
            Self::Recover(_) => "recover",
            Self::Replay(_) => "replay",
            Self::LongTermList(_) => "long_term_list",
            Self::LongTermDetail(_) => "long_term_detail",
            Self::LongTermMutate(_) => "long_term_mutate",
            Self::LongTermPolicy(_) => "long_term_policy",
            Self::TranscriptAttrWrite(_) => "transcript_attr_write",
            Self::Capabilities(_) => "capabilities",
            Self::Close(_) => "close",
        }
    }

    pub fn governed_safe_report(&self) -> Option<AdapterGovernedSafeReportV1> {
        match self {
            Self::Recall(report) => Some(AdapterGovernedSafeReportV1::Recall(
                AdapterGovernedRecallSafeReportV1 {
                    query: report.query.clone(),
                    temporal_operation: report.temporal_operation,
                    governed_recall: report.governed_public_report().clone(),
                },
            )),
            Self::Project(report) => Some(AdapterGovernedSafeReportV1::Project(
                AdapterGovernedProjectSafeReportV1 {
                    temporal_operation: report.governed_recall.authority().temporal_operation(),
                    projection_block: report.projection_block.clone(),
                    chars: report.chars,
                    agent_tool_hints: report.agent_tool_hints.clone(),
                    audit: report.audit.clone(),
                    governed_recall: report.governed_recall.clone(),
                },
            )),
            _ => None,
        }
    }
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
