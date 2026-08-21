use bm_sdk::{
    AgentToolHint, GovernedRecallPublicReportV1, MemoryCloseReport, MemoryCloseRequest,
    MemoryGovernancePolicyMutationReport, MemoryInspectionReport, MemoryInspectionRequest,
    MemoryLongTermDetailReport, MemoryLongTermDetailRequest, MemoryLongTermListReport,
    MemoryLongTermListRequest, MemoryLongTermMutationReport, MemoryLongTermMutationRequest,
    MemoryLongTermPolicyRequest, MemoryMaintenanceReport, MemoryMaintenanceRequest,
    MemoryProjectionReport, MemoryProjectionRequest, MemoryRecallReport, MemoryRecallRequest,
    MemoryRecallTemporalOperation, MemoryRecoverReport, MemoryRecoverRequest, MemoryReplayReport,
    MemoryReplayRequest, MemoryRuntime, MemoryTranscriptAttrWriteReport,
    MemoryTranscriptAttrWriteRequest, MemoryTurnFinalizeReport, MemoryTurnFinalizeRequest,
    MemoryWriteReport, MemoryWriteRequest, ProfileId, RuntimeBudgetLease,
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
    pub mutation_operation_id: Option<String>,
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
        let mutation_operation_id = match explicit_idempotency_key {
            Some(key) => {
                let key = key.trim();
                if key.is_empty() {
                    return Err(AdapterRequestIdentityError::EmptyExplicitIdempotencyKey);
                }
                Some(derived_idempotency_key("explicit", &[principal, key]))
            }
            None => None,
        };
        Ok(AdapterRequestIdentity {
            audit_id: format!("audit-{request_id}"),
            request_id,
            mutation_operation_id,
        })
    }
}

fn derived_idempotency_key(mode: &str, fields: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for field in std::iter::once("bm_adapter_request_identity_v1")
        .chain(std::iter::once(mode))
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
    FinalizeTurn,
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
    pub const ALL: [Self; 16] = [
        Self::Write,
        Self::FinalizeTurn,
        Self::Recall,
        Self::Project,
        Self::Maintain,
        Self::Inspect,
        Self::Recover,
        Self::Replay,
        Self::LongTermList,
        Self::LongTermDetail,
        Self::LongTermMutate,
        Self::LongTermPolicy,
        Self::TranscriptAttrWrite,
        Self::Capabilities,
        Self::Subscribe,
        Self::Close,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::FinalizeTurn => "finalize_turn",
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

    pub const fn requires_in_flight_reservation(self) -> bool {
        match self {
            Self::Write
            | Self::FinalizeTurn
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

    pub const fn mutation_reliability(self) -> AdapterMutationReliability {
        match self {
            Self::Write | Self::LongTermMutate => AdapterMutationReliability::DurableStoreReceipt,
            Self::FinalizeTurn
            | Self::Maintain
            | Self::Recover
            | Self::LongTermPolicy
            | Self::TranscriptAttrWrite
            | Self::Close => AdapterMutationReliability::ExplicitlyNonDurable,
            Self::Recall
            | Self::Project
            | Self::Inspect
            | Self::Replay
            | Self::LongTermList
            | Self::LongTermDetail
            | Self::Capabilities
            | Self::Subscribe => AdapterMutationReliability::NotMutation,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterMutationReliability {
    NotMutation,
    DurableStoreReceipt,
    ExplicitlyNonDurable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterMutationOperationCapability {
    pub operation: AdapterOperation,
    pub reliability: AdapterMutationReliability,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExternalAiMemoryProtocolVersion {
    #[serde(rename = "beetle-memory.external-ai.v1")]
    V1,
    #[serde(rename = "beetle-memory.external-ai.v2")]
    V2,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterProtocolPrivacyBinding {
    pub prompt_projection_allowed: bool,
    pub private_plane_projection_allowed: bool,
    pub operator_inspection_allowed: bool,
    pub export_allowed: bool,
    pub import_allowed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterProtocolCapabilityBinding {
    pub write_visible: bool,
    pub recall_visible: bool,
    pub projection_visible: bool,
    pub maintenance_visible: bool,
    pub inspection_visible: bool,
    pub replay_visible: bool,
    pub long_term_inspect_visible: bool,
    pub long_term_mutation_visible: bool,
    pub long_term_policy_visible: bool,
    pub transcript_replay_visible: bool,
    pub communication_adapter_visible: bool,
    pub cli_visible: bool,
    pub http_visible: bool,
    pub wss_visible: bool,
    pub mcp_visible: bool,
    pub a2a_visible: bool,
    pub mutation_operation_inventory: Vec<AdapterMutationOperationCapability>,
    pub mutation_receipt_policy: bm_sdk::MutationOperationReceiptQuota,
}

impl AdapterProtocolCapabilityBinding {
    pub fn from_runtime(runtime: &MemoryRuntime) -> Self {
        let catalog = runtime.capabilities();
        let retention_quota = runtime.retention_quota_report();
        Self {
            write_visible: catalog.write.visible,
            recall_visible: catalog.recall.visible,
            projection_visible: catalog.projection.visible,
            maintenance_visible: catalog.maintenance.visible,
            inspection_visible: catalog.inspection.visible,
            replay_visible: catalog.replay.visible,
            long_term_inspect_visible: catalog.long_term_control_inspect.visible,
            long_term_mutation_visible: catalog.long_term_control_mutation.visible,
            long_term_policy_visible: catalog.long_term_control_policy.visible,
            transcript_replay_visible: catalog.transcript_replay.visible,
            communication_adapter_visible: catalog.communication_adapter.visible,
            cli_visible: catalog.adapter.cli.visible,
            http_visible: catalog.adapter.http.visible,
            wss_visible: catalog.adapter.wss.visible,
            mcp_visible: catalog.adapter.mcp.visible,
            a2a_visible: catalog.adapter.a2a.visible,
            mutation_operation_inventory: AdapterOperation::ALL
                .into_iter()
                .map(|operation| AdapterMutationOperationCapability {
                    operation,
                    reliability: operation.mutation_reliability(),
                })
                .collect(),
            mutation_receipt_policy: retention_quota.mutation_operation_receipts,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterCapabilityReportV2 {
    pub profile: ProfileId,
    pub capabilities: AdapterProtocolCapabilityBinding,
    pub sdk_mutation_inventory: bm_sdk::MemoryMutationCapabilityCatalog,
}

impl AdapterCapabilityReportV2 {
    pub fn for_runtime(runtime: &MemoryRuntime) -> Self {
        Self {
            profile: runtime.config().profile,
            capabilities: AdapterProtocolCapabilityBinding::from_runtime(runtime),
            sdk_mutation_inventory: runtime.mutation_capabilities(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterProtocolRenderBudgetBinding {
    pub system_block_max_chars: usize,
    pub provider_prompt_max_chars: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterProtocolBinding {
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub agent_id: String,
    pub owner_id: String,
    pub channel: String,
    pub chat_id: String,
    pub conversation_id: String,
    pub profile: ProfileId,
    pub privacy: AdapterProtocolPrivacyBinding,
    pub capabilities: AdapterProtocolCapabilityBinding,
    pub render_budget: AdapterProtocolRenderBudgetBinding,
    pub budget_report_id: String,
}

impl AdapterProtocolBinding {
    pub fn for_runtime(runtime: &MemoryRuntime, lease: &RuntimeBudgetLease) -> Self {
        let config = runtime.config();
        let privacy = &config.privacy_policy;
        let render_budget = lease.report().projection_render_budget;
        Self {
            memory_space_id: runtime.memory_space_id().to_string(),
            mounted_subject_id: runtime.subject_id().to_string(),
            agent_id: runtime.identity().agent_id.clone(),
            owner_id: runtime.identity().owner_id.clone(),
            channel: runtime.scope().channel.clone(),
            chat_id: runtime.scope().chat_id.clone(),
            conversation_id: runtime.scope().conversation_id_or_chat_id().to_string(),
            profile: config.profile,
            privacy: AdapterProtocolPrivacyBinding {
                prompt_projection_allowed: privacy.prompt_projection_allowed,
                private_plane_projection_allowed: privacy.private_plane_projection_allowed,
                operator_inspection_allowed: privacy.operator_inspection_allowed,
                export_allowed: privacy.export_allowed,
                import_allowed: privacy.import_allowed,
            },
            capabilities: AdapterProtocolCapabilityBinding::from_runtime(runtime),
            render_budget: AdapterProtocolRenderBudgetBinding {
                system_block_max_chars: render_budget.system_block_max_chars,
                provider_prompt_max_chars: render_budget.provider_prompt_max_chars,
            },
            budget_report_id: lease.report_id().to_string(),
        }
    }

    pub(crate) fn mismatch_reason(
        &self,
        runtime: &MemoryRuntime,
        lease: &RuntimeBudgetLease,
    ) -> Option<&'static str> {
        let expected = Self::for_runtime(runtime, lease);
        if self.memory_space_id != expected.memory_space_id {
            return Some("memory_space_id_mismatch");
        }
        if self.mounted_subject_id != expected.mounted_subject_id {
            return Some("mounted_subject_id_mismatch");
        }
        if self.agent_id != expected.agent_id {
            return Some("agent_id_mismatch");
        }
        if self.owner_id != expected.owner_id {
            return Some("owner_id_mismatch");
        }
        if self.channel != expected.channel
            || self.chat_id != expected.chat_id
            || self.conversation_id != expected.conversation_id
        {
            return Some("conversation_scope_mismatch");
        }
        if self.profile != expected.profile {
            return Some("profile_mismatch");
        }
        if self.privacy != expected.privacy {
            return Some("privacy_policy_mismatch");
        }
        if self.capabilities != expected.capabilities {
            return Some("capability_snapshot_mismatch");
        }
        if self.render_budget != expected.render_budget {
            return Some("render_budget_mismatch");
        }
        if self.budget_report_id != expected.budget_report_id {
            return Some("budget_report_id_mismatch");
        }
        None
    }

    pub(crate) fn source_mismatch_reason(&self, source: &AdapterSource) -> Option<&'static str> {
        (source.agent_id != self.agent_id
            || source.owner_id != self.owner_id
            || source.channel != self.channel
            || source.chat_id != self.chat_id)
            .then_some("source_identity_mismatch")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterEnvelope<T> {
    pub protocol_version: ExternalAiMemoryProtocolVersion,
    pub runtime_binding: AdapterProtocolBinding,
    pub request_id: String,
    pub transport: TransportKind,
    pub mode: TransportMode,
    pub operation: AdapterOperation,
    pub source: AdapterSource,
    pub auth: AdapterAuthContext,
    pub mutation_operation_id: Option<String>,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        receipt: Option<AdapterMutationReceiptV1>,
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
    Replayed {
        request_id: String,
        audit_id: String,
        mutation_operation_id: String,
        receipt: AdapterMutationReceiptV1,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterMutationReceiptV1 {
    pub schema_version: u32,
    pub mutation_operation_id: String,
    pub operation_kind: bm_sdk::MemoryMutationOperationKind,
    pub transaction_id: String,
    pub effect: bm_sdk::MemoryMutationEffect,
    pub changed_count: u64,
    pub audit_record_id: String,
    pub committed_at_unix_secs: u64,
}

impl AdapterMutationReceiptV1 {
    pub fn from_operation_id(
        mutation_operation_id: impl Into<String>,
        receipt: &bm_sdk::MemoryMutationReceipt,
    ) -> Self {
        Self {
            schema_version: receipt.schema_version,
            mutation_operation_id: mutation_operation_id.into(),
            operation_kind: receipt.identity.operation_kind(),
            transaction_id: receipt.transaction_id.clone(),
            effect: receipt.effect,
            changed_count: receipt.changed_count,
            audit_record_id: receipt.audit_record_id.clone(),
            committed_at_unix_secs: receipt.committed_at_unix_secs,
        }
    }
}

pub enum AdapterCommand {
    Write(MemoryWriteRequest),
    FinalizeTurn(Box<MemoryTurnFinalizeRequest>),
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
            Self::FinalizeTurn(_) => AdapterOperation::FinalizeTurn,
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
            Self::FinalizeTurn(request) => {
                serde_json::to_vec(&AdapterTurnFinalizeFingerprint::from(request.as_ref()))?
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

#[derive(Serialize)]
struct AdapterTurnFinalizeFingerprint<'a> {
    turn: &'a bm_sdk::CanonicalTurnDelta,
    tool_calls: u32,
    runtime_skill_selected_ids: &'a [String],
    task_learning_selected_ids: &'a [String],
    reuse_outcome_note: &'a str,
    tool_usage_feedback: &'a Option<bm_sdk::AgentToolUsageFeedback>,
    pressure: bm_sdk::PressureLevel,
    mode_input: bm_sdk::RuntimeLifecycleModeInput,
}

impl<'a> From<&'a MemoryTurnFinalizeRequest> for AdapterTurnFinalizeFingerprint<'a> {
    fn from(request: &'a MemoryTurnFinalizeRequest) -> Self {
        Self {
            turn: &request.turn,
            tool_calls: request.tool_calls,
            runtime_skill_selected_ids: &request.runtime_skill_selected_ids,
            task_learning_selected_ids: &request.task_learning_selected_ids,
            reuse_outcome_note: &request.reuse_outcome_note,
            tool_usage_feedback: &request.tool_usage_feedback,
            pressure: request.pressure,
            mode_input: request.mode_input,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterTurnFinalizeReport {
    pub turn_id: String,
    pub session_committed: bool,
    pub transcript_committed: bool,
    pub maintenance_performed: bool,
    #[serde(rename = "memoryConsolidation")]
    pub memory_consolidation: AdapterMemoryConsolidationReport,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdapterMemoryConsolidationReport {
    pub state: bm_sdk::MemoryConsolidationState,
    pub job_id: Option<String>,
    pub reason: String,
}

impl AdapterTurnFinalizeReport {
    pub(crate) fn from_sdk(turn_id: String, report: MemoryTurnFinalizeReport) -> Self {
        Self {
            turn_id,
            session_committed: report.session_commit.committed,
            transcript_committed: report
                .transcript_commit
                .as_ref()
                .is_some_and(|commit| commit.committed),
            maintenance_performed: report.maintenance.is_some(),
            memory_consolidation: AdapterMemoryConsolidationReport {
                state: report.memory_consolidation.state,
                job_id: report.memory_consolidation.job_id,
                reason: report.memory_consolidation.reason,
            },
        }
    }
}

pub enum AdapterSdkReport {
    Write(Box<MemoryWriteReport>),
    FinalizeTurn(Box<AdapterTurnFinalizeReport>),
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
    Capabilities(Box<AdapterCapabilityReportV2>),
    Close(Box<MemoryCloseReport>),
}

impl AdapterSdkReport {
    pub const fn public_kind(&self) -> &'static str {
        match self {
            Self::Write(_) => "write",
            Self::FinalizeTurn(_) => "finalize_turn",
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
            Self::FinalizeTurn(_) => "FinalizeTurn",
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
