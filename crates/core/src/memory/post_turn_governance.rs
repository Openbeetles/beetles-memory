use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{Error, Result};

use super::{PrivateGardenGovernanceManifestEntry, SessionTurnCommitReport, TranscriptTurnRecord};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryWriteAuthority {
    RuntimeDeterministic,
    LlmPrivateGardenFreeform,
    LlmGovernedSemantic,
    SoulGovernedCore,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivateGardenAdmissionDecision {
    Applied,
    Skipped,
    BoundaryRejected,
    BoundaryTruncated,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostTurnPrivateGardenReport {
    pub attempted: bool,
    pub executed: bool,
    pub authority: MemoryWriteAuthority,
    pub admission: PrivateGardenAdmissionDecision,
    pub writes: usize,
    pub moves: usize,
    pub deletes: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub manifest: Vec<PrivateGardenGovernanceManifestEntry>,
    pub skipped_reason: Option<String>,
}

impl PostTurnPrivateGardenReport {
    pub fn skipped(reason: impl Into<String>) -> Self {
        Self {
            attempted: false,
            executed: false,
            authority: MemoryWriteAuthority::LlmPrivateGardenFreeform,
            admission: PrivateGardenAdmissionDecision::Skipped,
            writes: 0,
            moves: 0,
            deletes: 0,
            manifest: Vec::new(),
            skipped_reason: Some(reason.into()),
        }
    }

    pub fn no_change(reason: impl Into<String>) -> Self {
        Self {
            attempted: true,
            executed: false,
            authority: MemoryWriteAuthority::LlmPrivateGardenFreeform,
            admission: PrivateGardenAdmissionDecision::Skipped,
            writes: 0,
            moves: 0,
            deletes: 0,
            manifest: Vec::new(),
            skipped_reason: Some(reason.into()),
        }
    }

    pub fn applied(writes: usize, moves: usize, deletes: usize) -> Self {
        Self::applied_with_manifest(writes, moves, deletes, Vec::new())
    }

    pub fn applied_with_manifest(
        writes: usize,
        moves: usize,
        deletes: usize,
        manifest: Vec<PrivateGardenGovernanceManifestEntry>,
    ) -> Self {
        Self {
            attempted: true,
            executed: true,
            authority: MemoryWriteAuthority::LlmPrivateGardenFreeform,
            admission: PrivateGardenAdmissionDecision::Applied,
            writes,
            moves,
            deletes,
            manifest,
            skipped_reason: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryWriteDomain {
    Program,
    Subject,
    Procedural,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernedWriteDecision {
    Accepted,
    Rejected,
    Merged,
    Superseded,
    Deferred,
    NotApplicable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryPlaneGovernanceReport {
    pub domain: MemoryWriteDomain,
    pub plane: String,
    pub authority: MemoryWriteAuthority,
    pub decision: GovernedWriteDecision,
    pub reason: String,
    pub evidence_refs: Vec<String>,
    pub privacy_decision: String,
    pub profile_decision: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoulCandidateDisposition {
    HandedOff,
    RejectedAsMemoryOnly,
    Deferred,
    NotApplicable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoulCandidateHandoffReport {
    pub surface: String,
    pub disposition: SoulCandidateDisposition,
    pub existing_gate: String,
    pub reason: String,
    pub evidence_refs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostTurnSemanticGovernanceReport {
    pub attempted: bool,
    pub executed: bool,
    pub skipped_reason: Option<String>,
    pub proposal_count: usize,
    pub accepted_count: usize,
    pub rejected_count: usize,
    pub deferred_count: usize,
    pub plane_reports: Vec<MemoryPlaneGovernanceReport>,
    pub soul_candidate_handoffs: Vec<SoulCandidateHandoffReport>,
}

impl PostTurnSemanticGovernanceReport {
    pub fn skipped(reason: impl Into<String>) -> Self {
        Self {
            attempted: false,
            executed: false,
            skipped_reason: Some(reason.into()),
            proposal_count: 0,
            accepted_count: 0,
            rejected_count: 0,
            deferred_count: 0,
            plane_reports: Vec::new(),
            soul_candidate_handoffs: Vec::new(),
        }
    }

    pub fn deferred(reason: impl Into<String>, plane: impl Into<String>) -> Self {
        let reason = reason.into();
        Self {
            attempted: true,
            executed: false,
            skipped_reason: Some(reason.clone()),
            proposal_count: 1,
            accepted_count: 0,
            rejected_count: 0,
            deferred_count: 1,
            plane_reports: vec![MemoryPlaneGovernanceReport {
                domain: MemoryWriteDomain::Program,
                plane: plane.into(),
                authority: MemoryWriteAuthority::RuntimeDeterministic,
                decision: GovernedWriteDecision::Deferred,
                reason,
                evidence_refs: Vec::new(),
                privacy_decision: "deferred_without_semantic_mutation".to_string(),
                profile_decision: "runtime_admission_or_service_unavailable".to_string(),
            }],
            soul_candidate_handoffs: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeferredGovernanceJobStatus {
    Pending,
    Retrying,
    Failed,
    Terminal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeferredGovernanceJobSummary {
    pub job_id: String,
    pub idempotency_key: String,
    pub status: DeferredGovernanceJobStatus,
    pub memory_space_id: String,
    pub subject_id: String,
    pub channel: String,
    pub chat_id: String,
    pub conversation_id: Option<String>,
    pub turn_id: String,
    pub candidate_ids: Vec<String>,
    pub reason: String,
    pub retry_policy: String,
    pub created_at: u64,
    pub attempts: u32,
    pub last_error: Option<String>,
}

impl DeferredGovernanceJobSummary {
    pub fn from_job(job: &PostTurnGovernanceJobV2) -> Self {
        let status = match job.status {
            PostTurnGovernanceJobStatusV2::Pending
            | PostTurnGovernanceJobStatusV2::BlockedConfiguration
            | PostTurnGovernanceJobStatusV2::BlockedCapability
            | PostTurnGovernanceJobStatusV2::BlockedPolicy => DeferredGovernanceJobStatus::Pending,
            PostTurnGovernanceJobStatusV2::Leased | PostTurnGovernanceJobStatusV2::RetryWaiting => {
                DeferredGovernanceJobStatus::Retrying
            }
            PostTurnGovernanceJobStatusV2::DeadLetter => DeferredGovernanceJobStatus::Failed,
            PostTurnGovernanceJobStatusV2::Succeeded | PostTurnGovernanceJobStatusV2::Cancelled => {
                DeferredGovernanceJobStatus::Terminal
            }
        };
        Self {
            job_id: job.job_id.clone(),
            idempotency_key: job.idempotency_key.clone(),
            status,
            memory_space_id: job.identity.memory_space_id.clone(),
            subject_id: job.identity.mounted_subject_id.clone(),
            channel: job.identity.channel_id.clone(),
            chat_id: job.identity.chat_id.clone(),
            conversation_id: Some(job.identity.conversation_id.clone()),
            turn_id: job.identity.turn_id.clone(),
            candidate_ids: job.candidate_ids.clone(),
            reason: job
                .blocking_reason
                .clone()
                .unwrap_or_else(|| "v2_durable_governance_intent".to_string()),
            retry_policy: "bounded_exponential_backoff".to_string(),
            created_at: job.created_at,
            attempts: job.attempt_count,
            last_error: job.last_error_class.map(|error| error.as_str().to_string()),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeferredGovernanceQueueReport {
    pub total: usize,
    pub pending: usize,
    pub retrying: usize,
    pub failed: usize,
    pub terminal: usize,
    pub oldest_pending_at: Option<u64>,
    pub newest_pending_at: Option<u64>,
    pub recent_jobs: Vec<DeferredGovernanceJobSummary>,
}

pub fn build_deferred_governance_queue_report(
    jobs: &[PostTurnGovernanceJobV2],
    recent_limit: usize,
) -> DeferredGovernanceQueueReport {
    let mut report = DeferredGovernanceQueueReport {
        total: jobs.len(),
        ..DeferredGovernanceQueueReport::default()
    };
    for job in jobs {
        match DeferredGovernanceJobSummary::from_job(job).status {
            DeferredGovernanceJobStatus::Pending => {
                report.pending = report.pending.saturating_add(1)
            }
            DeferredGovernanceJobStatus::Retrying => {
                report.retrying = report.retrying.saturating_add(1)
            }
            DeferredGovernanceJobStatus::Failed => report.failed = report.failed.saturating_add(1),
            DeferredGovernanceJobStatus::Terminal => {
                report.terminal = report.terminal.saturating_add(1)
            }
        }
        if matches!(
            DeferredGovernanceJobSummary::from_job(job).status,
            DeferredGovernanceJobStatus::Pending | DeferredGovernanceJobStatus::Retrying
        ) {
            report.oldest_pending_at = Some(
                report
                    .oldest_pending_at
                    .map_or(job.created_at, |value| value.min(job.created_at)),
            );
            report.newest_pending_at = Some(
                report
                    .newest_pending_at
                    .map_or(job.created_at, |value| value.max(job.created_at)),
            );
        }
    }
    let mut recent_jobs = jobs
        .iter()
        .map(DeferredGovernanceJobSummary::from_job)
        .collect::<Vec<_>>();
    recent_jobs.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| right.job_id.cmp(&left.job_id))
    });
    recent_jobs.truncate(recent_limit.max(1));
    report.recent_jobs = recent_jobs;
    report
}

pub const POST_TURN_GOVERNANCE_JOB_SCHEMA_VERSION: u32 = 2;
pub const POST_TURN_GOVERNANCE_SCOPE_INDEX_SCHEMA_VERSION: u32 = 2;
pub const POST_TURN_GOVERNANCE_JOB_NAMESPACE: &str = "post_turn_governance_jobs";
pub const POST_TURN_GOVERNANCE_SCOPE_INDEX_NAMESPACE: &str = "post_turn_governance_scope_indexes";
pub const MAX_POST_TURN_GOVERNANCE_ACTIVE_JOBS: usize = 256;
pub const MAX_POST_TURN_GOVERNANCE_RECENT_TERMINAL_JOBS: usize = 256;
pub const MAX_POST_TURN_GOVERNANCE_RECONCILIATION_CURSORS: usize = 256;
pub const MAX_POST_TURN_GOVERNANCE_ERROR_CHARS: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostTurnGovernanceIdentityV2 {
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub channel_id: String,
    pub chat_id: String,
    pub conversation_id: String,
    pub turn_id: String,
}

impl PostTurnGovernanceIdentityV2 {
    pub fn new(
        memory_space_id: impl Into<String>,
        mounted_subject_id: impl Into<String>,
        channel_id: impl Into<String>,
        chat_id: impl Into<String>,
        conversation_id: impl Into<String>,
        turn_id: impl Into<String>,
    ) -> Result<Self> {
        let identity = Self {
            memory_space_id: memory_space_id.into(),
            mounted_subject_id: mounted_subject_id.into(),
            channel_id: channel_id.into(),
            chat_id: chat_id.into(),
            conversation_id: conversation_id.into(),
            turn_id: turn_id.into(),
        };
        identity.validate()?;
        Ok(identity)
    }

    pub fn validate(&self) -> Result<()> {
        for (name, value) in [
            ("memory_space_id", self.memory_space_id.as_str()),
            ("mounted_subject_id", self.mounted_subject_id.as_str()),
            ("channel_id", self.channel_id.as_str()),
            ("chat_id", self.chat_id.as_str()),
            ("conversation_id", self.conversation_id.as_str()),
            ("turn_id", self.turn_id.as_str()),
        ] {
            if value.is_empty() || value.trim() != value {
                return Err(Error::invalid_input(
                    "post_turn_governance_identity",
                    format!("{name} must be exact and non-empty"),
                ));
            }
        }
        Ok(())
    }

    pub fn job_id(&self) -> String {
        format!(
            "ptgj2:{}",
            governance_digest(&[
                b"post_turn_governance_job_v2",
                self.memory_space_id.as_bytes(),
                self.mounted_subject_id.as_bytes(),
                self.channel_id.as_bytes(),
                self.chat_id.as_bytes(),
                self.conversation_id.as_bytes(),
                self.turn_id.as_bytes(),
            ])
        )
    }

    pub fn job_key(&self) -> String {
        self.job_id()
    }

    pub fn scope_id(&self) -> String {
        format!(
            "ptgsi2:{}",
            governance_digest(&[
                b"post_turn_governance_runtime_scope_index_v2",
                self.memory_space_id.as_bytes(),
                self.mounted_subject_id.as_bytes(),
                self.channel_id.as_bytes(),
                self.chat_id.as_bytes(),
            ])
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PostTurnGovernanceJobStatusV2 {
    Pending,
    Leased,
    RetryWaiting,
    BlockedConfiguration,
    BlockedCapability,
    BlockedPolicy,
    Succeeded,
    Cancelled,
    DeadLetter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PostTurnGovernanceErrorClassV2 {
    ServiceUnavailable,
    Timeout,
    RateLimited,
    MalformedModelOutput,
    IdentityMismatch,
    TranscriptDigestMismatch,
    SchemaViolation,
}

impl PostTurnGovernanceErrorClassV2 {
    pub const fn is_retryable(self) -> bool {
        matches!(
            self,
            Self::ServiceUnavailable
                | Self::Timeout
                | Self::RateLimited
                | Self::MalformedModelOutput
        )
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ServiceUnavailable => "service_unavailable",
            Self::Timeout => "timeout",
            Self::RateLimited => "rate_limited",
            Self::MalformedModelOutput => "malformed_model_output",
            Self::IdentityMismatch => "identity_mismatch",
            Self::TranscriptDigestMismatch => "transcript_digest_mismatch",
            Self::SchemaViolation => "schema_violation",
        }
    }
}

impl PostTurnGovernanceJobStatusV2 {
    pub const fn is_active(self) -> bool {
        matches!(
            self,
            Self::Pending
                | Self::Leased
                | Self::RetryWaiting
                | Self::BlockedConfiguration
                | Self::BlockedCapability
                | Self::BlockedPolicy
        )
    }

    pub const fn is_terminal(self) -> bool {
        !self.is_active()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostTurnGovernanceAttemptAuthorityV2 {
    pub binding_id: String,
    pub config_revision: u64,
    pub model_id: String,
    pub privacy_revision: u64,
    pub privacy_digest: String,
    pub transcript_lifecycle_revision: u64,
    pub disclosure_authority_digest: String,
}

impl PostTurnGovernanceAttemptAuthorityV2 {
    pub fn validate(&self) -> Result<()> {
        if self.binding_id.trim().is_empty()
            || self.model_id.trim().is_empty()
            || self.config_revision == 0
            || self.privacy_revision == 0
            || self.transcript_lifecycle_revision == 0
            || !is_sha256_digest(&self.privacy_digest)
            || !is_sha256_digest(&self.disclosure_authority_digest)
        {
            return Err(Error::invalid_input(
                "post_turn_governance_attempt_authority",
                "attempt authority is incomplete or invalid",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostTurnGovernanceReceiptV2 {
    pub semantic_transaction_id: String,
    pub mutation_plan_digest: String,
    pub memory_post_image_digest: String,
    pub completed_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostTurnGovernanceJobV2 {
    pub schema_version: u32,
    pub job_id: String,
    pub idempotency_key: String,
    pub scope_index_key: String,
    pub identity: PostTurnGovernanceIdentityV2,
    pub transcript_sequence: u64,
    pub transcript_digest: String,
    pub runtime_binding_digest: String,
    pub governance_contract_version: u32,
    pub governance_model_policy_revision: u64,
    pub pinned_privacy_revision: u64,
    pub pinned_privacy_digest: String,
    pub candidate_ids: Vec<String>,
    pub tool_call_count: u32,
    pub status: PostTurnGovernanceJobStatusV2,
    pub state_revision: u64,
    pub attempt_count: u32,
    pub max_attempts: u32,
    pub next_attempt_at: Option<u64>,
    pub lease_owner: Option<String>,
    pub lease_until: Option<u64>,
    pub lease_epoch: u64,
    pub attempt_authority: Option<PostTurnGovernanceAttemptAuthorityV2>,
    pub blocking_reason: Option<String>,
    pub last_error_class: Option<PostTurnGovernanceErrorClassV2>,
    pub receipt: Option<PostTurnGovernanceReceiptV2>,
    pub created_at: u64,
    pub updated_at: u64,
    pub terminal_at: Option<u64>,
}

impl PostTurnGovernanceJobV2 {
    #[allow(clippy::too_many_arguments)]
    pub fn pending(
        identity: PostTurnGovernanceIdentityV2,
        transcript_sequence: u64,
        transcript_digest: impl Into<String>,
        runtime_binding_digest: impl Into<String>,
        governance_model_policy_revision: u64,
        pinned_privacy_revision: u64,
        pinned_privacy_digest: impl Into<String>,
        candidate_ids: Vec<String>,
        tool_call_count: u32,
        max_attempts: u32,
        now_secs: u64,
    ) -> Result<Self> {
        let job_id = identity.job_id();
        let job = Self {
            schema_version: POST_TURN_GOVERNANCE_JOB_SCHEMA_VERSION,
            idempotency_key: job_id.clone(),
            scope_index_key: identity.scope_id(),
            job_id,
            identity,
            transcript_sequence,
            transcript_digest: transcript_digest.into(),
            runtime_binding_digest: runtime_binding_digest.into(),
            governance_contract_version: POST_TURN_GOVERNANCE_JOB_SCHEMA_VERSION,
            governance_model_policy_revision,
            pinned_privacy_revision,
            pinned_privacy_digest: pinned_privacy_digest.into(),
            candidate_ids,
            tool_call_count,
            status: PostTurnGovernanceJobStatusV2::Pending,
            state_revision: 1,
            attempt_count: 0,
            max_attempts,
            next_attempt_at: Some(now_secs),
            lease_owner: None,
            lease_until: None,
            lease_epoch: 0,
            attempt_authority: None,
            blocking_reason: None,
            last_error_class: None,
            receipt: None,
            created_at: now_secs,
            updated_at: now_secs,
            terminal_at: None,
        };
        job.validate()?;
        Ok(job)
    }

    pub fn validate(&self) -> Result<()> {
        self.identity.validate()?;
        if self.schema_version != POST_TURN_GOVERNANCE_JOB_SCHEMA_VERSION
            || self.governance_contract_version != POST_TURN_GOVERNANCE_JOB_SCHEMA_VERSION
            || self.job_id != self.identity.job_id()
            || self.idempotency_key != self.job_id
            || self.scope_index_key != self.identity.scope_id()
            || self.transcript_sequence == 0
            || !is_sha256_digest(&self.transcript_digest)
            || !is_sha256_digest(&self.runtime_binding_digest)
            || self.governance_model_policy_revision == 0
            || self.pinned_privacy_revision == 0
            || !is_sha256_digest(&self.pinned_privacy_digest)
            || self.state_revision == 0
            || self.max_attempts == 0
            || self.attempt_count > self.max_attempts
            || self.created_at == 0
            || self.updated_at < self.created_at
        {
            return Err(Error::invalid_input(
                "post_turn_governance_job",
                "job identity, authority, revision, or timing is invalid",
            ));
        }
        if self.candidate_ids.len() > MAX_POST_TURN_GOVERNANCE_ACTIVE_JOBS
            || self
                .candidate_ids
                .iter()
                .any(|value| value.trim().is_empty())
            || self.blocking_reason.as_deref().is_some_and(|value| {
                value.trim().is_empty()
                    || value.chars().count() > MAX_POST_TURN_GOVERNANCE_ERROR_CHARS
            })
        {
            return Err(Error::invalid_input(
                "post_turn_governance_job",
                "job metadata exceeds its bounded contract",
            ));
        }
        let leased = self.status == PostTurnGovernanceJobStatusV2::Leased;
        if leased
            != (self.lease_owner.is_some()
                && self.lease_until.is_some()
                && self.attempt_authority.is_some())
        {
            return Err(Error::invalid_input(
                "post_turn_governance_job",
                "lease fields differ from job status",
            ));
        }
        if let Some(authority) = &self.attempt_authority {
            authority.validate()?;
        }
        if self.status.is_terminal()
            != (self.terminal_at.is_some()
                && self.lease_owner.is_none()
                && self.lease_until.is_none())
        {
            return Err(Error::invalid_input(
                "post_turn_governance_job",
                "terminal fields differ from job status",
            ));
        }
        if (self.status == PostTurnGovernanceJobStatusV2::Succeeded) != self.receipt.is_some() {
            return Err(Error::invalid_input(
                "post_turn_governance_job",
                "success status requires one deterministic receipt",
            ));
        }
        if let Some(receipt) = &self.receipt {
            if receipt.semantic_transaction_id.trim().is_empty()
                || !is_sha256_digest(&receipt.mutation_plan_digest)
                || !is_sha256_digest(&receipt.memory_post_image_digest)
                || receipt.completed_at == 0
            {
                return Err(Error::invalid_input(
                    "post_turn_governance_job",
                    "completion receipt is invalid",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostTurnGovernanceJobRefV1 {
    pub job_id: String,
    pub job_key: String,
    pub state_revision: u64,
    pub status: PostTurnGovernanceJobStatusV2,
    pub next_attempt_at: Option<u64>,
    pub lease_until: Option<u64>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl PostTurnGovernanceJobRefV1 {
    pub fn from_job(job: &PostTurnGovernanceJobV2) -> Self {
        Self {
            job_id: job.job_id.clone(),
            job_key: job.job_id.clone(),
            state_revision: job.state_revision,
            status: job.status,
            next_attempt_at: job.next_attempt_at,
            lease_until: job.lease_until,
            created_at: job.created_at,
            updated_at: job.updated_at,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostTurnGovernanceReconciliationCursorV1 {
    pub conversation_id: String,
    pub sequence: u64,
    pub turn_id: String,
}

impl PostTurnGovernanceReconciliationCursorV1 {
    pub fn validate(&self) -> Result<()> {
        if self.conversation_id.trim().is_empty()
            || self.conversation_id.trim() != self.conversation_id
            || self.sequence == 0
            || self.turn_id.trim().is_empty()
            || self.turn_id.trim() != self.turn_id
        {
            return Err(Error::invalid_input(
                "post_turn_governance_reconciliation_cursor",
                "reconciliation cursor identity or sequence is invalid",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostTurnGovernanceScopeIndexV2 {
    pub schema_version: u32,
    pub scope_index_key: String,
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub channel_id: String,
    pub chat_id: String,
    pub index_revision: u64,
    pub active_jobs: Vec<PostTurnGovernanceJobRefV1>,
    pub recent_terminal_jobs: Vec<PostTurnGovernanceJobRefV1>,
    pub reconciliation_cursors: Vec<PostTurnGovernanceReconciliationCursorV1>,
    pub updated_at: u64,
}

impl PostTurnGovernanceScopeIndexV2 {
    pub fn empty(identity: &PostTurnGovernanceIdentityV2, now_secs: u64) -> Self {
        Self {
            schema_version: POST_TURN_GOVERNANCE_SCOPE_INDEX_SCHEMA_VERSION,
            scope_index_key: identity.scope_id(),
            memory_space_id: identity.memory_space_id.clone(),
            mounted_subject_id: identity.mounted_subject_id.clone(),
            channel_id: identity.channel_id.clone(),
            chat_id: identity.chat_id.clone(),
            index_revision: 1,
            active_jobs: Vec::new(),
            recent_terminal_jobs: Vec::new(),
            reconciliation_cursors: Vec::new(),
            updated_at: now_secs,
        }
    }

    pub fn reconciliation_cursor(
        &self,
        conversation_id: &str,
    ) -> Option<&PostTurnGovernanceReconciliationCursorV1> {
        self.reconciliation_cursors
            .iter()
            .find(|cursor| cursor.conversation_id == conversation_id)
    }

    pub fn set_reconciliation_cursor(
        &mut self,
        cursor: PostTurnGovernanceReconciliationCursorV1,
    ) -> Result<()> {
        cursor.validate()?;
        if let Some(existing) = self
            .reconciliation_cursors
            .iter_mut()
            .find(|existing| existing.conversation_id == cursor.conversation_id)
        {
            if cursor.sequence <= existing.sequence {
                return Err(Error::conflict(
                    "post_turn_governance_reconcile",
                    "reconciliation cursor must advance monotonically",
                ));
            }
            *existing = cursor;
        } else {
            if self.reconciliation_cursors.len() >= MAX_POST_TURN_GOVERNANCE_RECONCILIATION_CURSORS
            {
                return Err(Error::config(
                    "post_turn_governance_reconcile",
                    "runtime scope reconciliation cursor budget is exhausted",
                ));
            }
            self.reconciliation_cursors.push(cursor);
            self.reconciliation_cursors
                .sort_by(|left, right| left.conversation_id.cmp(&right.conversation_id));
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        let identity = PostTurnGovernanceIdentityV2::new(
            &self.memory_space_id,
            &self.mounted_subject_id,
            &self.channel_id,
            &self.chat_id,
            "runtime-scope-validation-conversation",
            "scope-validation-turn",
        )?;
        if self.schema_version != POST_TURN_GOVERNANCE_SCOPE_INDEX_SCHEMA_VERSION
            || self.scope_index_key != identity.scope_id()
            || self.index_revision == 0
            || self.updated_at == 0
            || self.reconciliation_cursors.len() > MAX_POST_TURN_GOVERNANCE_RECONCILIATION_CURSORS
            || self
                .reconciliation_cursors
                .iter()
                .any(|cursor| cursor.validate().is_err())
            || self.active_jobs.len() > MAX_POST_TURN_GOVERNANCE_ACTIVE_JOBS
            || self.recent_terminal_jobs.len() > MAX_POST_TURN_GOVERNANCE_RECENT_TERMINAL_JOBS
            || self.active_jobs.iter().any(|job| {
                !job.status.is_active()
                    || job.job_id != job.job_key
                    || job.state_revision == 0
                    || job.updated_at < job.created_at
            })
            || self.recent_terminal_jobs.iter().any(|job| {
                !job.status.is_terminal()
                    || job.job_id != job.job_key
                    || job.state_revision == 0
                    || job.updated_at < job.created_at
            })
        {
            return Err(Error::invalid_input(
                "post_turn_governance_scope_index",
                "scope index identity, revision, or active refs are invalid",
            ));
        }
        let mut ids = std::collections::BTreeSet::new();
        if self
            .active_jobs
            .iter()
            .chain(self.recent_terminal_jobs.iter())
            .any(|job| !ids.insert(&job.job_id))
        {
            return Err(Error::invalid_input(
                "post_turn_governance_scope_index",
                "scope index contains duplicate job refs",
            ));
        }
        let mut conversation_ids = std::collections::BTreeSet::new();
        if self
            .reconciliation_cursors
            .iter()
            .any(|cursor| !conversation_ids.insert(&cursor.conversation_id))
        {
            return Err(Error::invalid_input(
                "post_turn_governance_scope_index",
                "scope index contains duplicate reconciliation cursors",
            ));
        }
        Ok(())
    }
}

fn governance_digest(fields: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    for field in fields {
        hasher.update((field.len() as u64).to_be_bytes());
        hasher.update(field);
    }
    let digest = hasher.finalize();
    digest[..28]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn is_sha256_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn post_turn_governance_transcript_digest(record: &TranscriptTurnRecord) -> Result<String> {
    let bytes = serde_json::to_vec(record).map_err(|error| {
        Error::config("post_turn_governance_transcript_digest", error.to_string())
    })?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostTurnMemoryGovernanceReport<MaintenanceReport, LifecycleReport> {
    pub session_commit: SessionTurnCommitReport,
    pub maintenance: Option<MaintenanceReport>,
    pub private_garden_self_work: PostTurnPrivateGardenReport,
    pub semantic_governance: PostTurnSemanticGovernanceReport,
    pub lifecycle_report: LifecycleReport,
}
