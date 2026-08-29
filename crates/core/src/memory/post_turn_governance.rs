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
    pub fn from_job(job: &PostTurnGovernanceJobV3) -> Self {
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
                .unwrap_or_else(|| "v3_durable_governance_intent".to_string()),
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
    jobs: &[PostTurnGovernanceJobV3],
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

pub const POST_TURN_GOVERNANCE_JOB_SCHEMA_VERSION: u32 = 3;
pub const POST_TURN_GOVERNANCE_SCOPE_INDEX_SCHEMA_VERSION: u32 = 3;
pub const POST_TURN_GOVERNANCE_PRIVACY_AUTHORITY_SCHEMA_VERSION: u32 = 1;
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
pub enum PostTurnGovernanceExecutionBindingV1 {
    Unbound,
    Bound {
        binding_id: String,
        binding_revision: u64,
    },
}

pub const POST_TURN_GOVERNANCE_BINDING_SNAPSHOT_NAMESPACE: &str =
    "post_turn_governance_binding_snapshots";
pub const POST_TURN_GOVERNANCE_BINDING_SNAPSHOT_SCHEMA_VERSION: u32 = 1;
pub const POST_TURN_GOVERNANCE_BINDING_REVISION_INDEX_NAMESPACE: &str =
    "post_turn_governance_binding_revision_indexes";
pub const POST_TURN_GOVERNANCE_BINDING_REVISION_INDEX_SCHEMA_VERSION: u32 = 1;
pub const MAX_POST_TURN_GOVERNANCE_BINDING_REVISIONS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PostTurnGovernanceProviderProtocolV1 {
    OpenAiCompatible,
    OllamaNative,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostTurnGovernanceBindingSnapshotV1 {
    pub schema_version: u32,
    pub binding_id: String,
    pub binding_revision: u64,
    pub source_owner_id: String,
    pub source_config_id: String,
    pub source_revision: u64,
    pub protocol: PostTurnGovernanceProviderProtocolV1,
    pub endpoint: String,
    pub model_id: String,
    pub credential_reference: Option<String>,
    pub request_timeout_ms: u64,
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub provider_permission_generation: u64,
    pub canonical_digest: String,
    pub created_at: u64,
}

impl PostTurnGovernanceBindingSnapshotV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        source_owner_id: impl Into<String>,
        source_config_id: impl Into<String>,
        source_revision: u64,
        protocol: PostTurnGovernanceProviderProtocolV1,
        endpoint: impl Into<String>,
        model_id: impl Into<String>,
        credential_reference: Option<String>,
        request_timeout_ms: u64,
        max_input_tokens: u64,
        max_output_tokens: u64,
        provider_permission_generation: u64,
        created_at: u64,
    ) -> Result<Self> {
        let source_owner_id = source_owner_id.into();
        let source_config_id = source_config_id.into();
        let endpoint = endpoint.into();
        let model_id = model_id.into();
        let binding_id = governance_binding_id(&source_owner_id, &source_config_id)?;
        let mut snapshot = Self {
            schema_version: POST_TURN_GOVERNANCE_BINDING_SNAPSHOT_SCHEMA_VERSION,
            binding_id,
            binding_revision: source_revision,
            source_owner_id,
            source_config_id,
            source_revision,
            protocol,
            endpoint,
            model_id,
            credential_reference,
            request_timeout_ms,
            max_input_tokens,
            max_output_tokens,
            provider_permission_generation,
            canonical_digest: String::new(),
            created_at,
        };
        snapshot.canonical_digest = snapshot.compute_canonical_digest()?;
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn storage_key(&self) -> String {
        format!("{}:{}", self.binding_id, self.binding_revision)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != POST_TURN_GOVERNANCE_BINDING_SNAPSHOT_SCHEMA_VERSION
            || self.binding_id
                != governance_binding_id(&self.source_owner_id, &self.source_config_id)?
            || self.binding_revision == 0
            || self.binding_revision != self.source_revision
            || !is_canonical_governance_text(&self.endpoint)
            || !is_canonical_governance_text(&self.model_id)
            || self
                .credential_reference
                .as_deref()
                .is_some_and(|value| !is_canonical_governance_text(value))
            || self.request_timeout_ms == 0
            || self.max_input_tokens == 0
            || self.max_output_tokens == 0
            || self.provider_permission_generation == 0
            || self.created_at == 0
            || self.canonical_digest != self.compute_canonical_digest()?
        {
            return Err(Error::invalid_input(
                "post_turn_governance_binding_snapshot",
                "binding snapshot identity, source, budgets, or digest is invalid",
            ));
        }
        Ok(())
    }

    fn compute_canonical_digest(&self) -> Result<String> {
        if !is_canonical_governance_text(&self.source_owner_id)
            || !is_canonical_governance_text(&self.source_config_id)
        {
            return Err(Error::invalid_input(
                "post_turn_governance_binding_snapshot",
                "binding source identity must be canonical",
            ));
        }
        let mut payload = b"bm-governance-binding-snapshot-v1\0".to_vec();
        for value in [
            self.source_owner_id.as_bytes(),
            self.source_config_id.as_bytes(),
            self.endpoint.as_bytes(),
            self.model_id.as_bytes(),
            self.credential_reference
                .as_deref()
                .unwrap_or("")
                .as_bytes(),
        ] {
            push_receipt_field(&mut payload, value);
        }
        payload.extend_from_slice(&self.source_revision.to_be_bytes());
        payload.push(match self.protocol {
            PostTurnGovernanceProviderProtocolV1::OpenAiCompatible => 1,
            PostTurnGovernanceProviderProtocolV1::OllamaNative => 2,
        });
        payload.extend_from_slice(&self.request_timeout_ms.to_be_bytes());
        payload.extend_from_slice(&self.max_input_tokens.to_be_bytes());
        payload.extend_from_slice(&self.max_output_tokens.to_be_bytes());
        payload.extend_from_slice(&self.provider_permission_generation.to_be_bytes());
        Ok(format!("sha256:{:x}", Sha256::digest(payload)))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostTurnGovernanceBindingRevisionRefV1 {
    pub binding_revision: u64,
    pub canonical_digest: String,
    pub created_at: u64,
    pub referenced: bool,
}

impl PostTurnGovernanceBindingRevisionRefV1 {
    pub fn from_snapshot(snapshot: &PostTurnGovernanceBindingSnapshotV1) -> Self {
        Self {
            binding_revision: snapshot.binding_revision,
            canonical_digest: snapshot.canonical_digest.clone(),
            created_at: snapshot.created_at,
            referenced: false,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.binding_revision == 0
            || !is_sha256_digest(&self.canonical_digest)
            || self.created_at == 0
        {
            return Err(Error::invalid_input(
                "post_turn_governance_binding_revision_ref",
                "binding revision reference is invalid",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostTurnGovernanceBindingRevisionIndexV1 {
    pub schema_version: u32,
    pub binding_id: String,
    pub source_owner_id: String,
    pub source_config_id: String,
    pub revisions: Vec<PostTurnGovernanceBindingRevisionRefV1>,
    pub index_revision: u64,
    pub updated_at: u64,
}

impl PostTurnGovernanceBindingRevisionIndexV1 {
    pub fn empty(snapshot: &PostTurnGovernanceBindingSnapshotV1) -> Self {
        Self {
            schema_version: POST_TURN_GOVERNANCE_BINDING_REVISION_INDEX_SCHEMA_VERSION,
            binding_id: snapshot.binding_id.clone(),
            source_owner_id: snapshot.source_owner_id.clone(),
            source_config_id: snapshot.source_config_id.clone(),
            revisions: Vec::new(),
            index_revision: 1,
            updated_at: snapshot.created_at,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != POST_TURN_GOVERNANCE_BINDING_REVISION_INDEX_SCHEMA_VERSION
            || self.binding_id
                != governance_binding_id(&self.source_owner_id, &self.source_config_id)?
            || self.revisions.is_empty()
            || self.revisions.len() > MAX_POST_TURN_GOVERNANCE_BINDING_REVISIONS
            || self.index_revision == 0
            || self.updated_at == 0
        {
            return Err(Error::invalid_input(
                "post_turn_governance_binding_revision_index",
                "binding revision index identity, capacity, or revision is invalid",
            ));
        }
        let mut previous = 0_u64;
        for revision in &self.revisions {
            revision.validate()?;
            if revision.binding_revision <= previous {
                return Err(Error::invalid_input(
                    "post_turn_governance_binding_revision_index",
                    "binding revisions must be unique and ascending",
                ));
            }
            previous = revision.binding_revision;
        }
        Ok(())
    }
}

fn governance_binding_id(source_owner_id: &str, source_config_id: &str) -> Result<String> {
    if !is_canonical_governance_text(source_owner_id)
        || !is_canonical_governance_text(source_config_id)
    {
        return Err(Error::invalid_input(
            "post_turn_governance_binding_snapshot",
            "binding source identity must be canonical",
        ));
    }
    let mut payload = b"bm-governance-binding-owner-v1\0".to_vec();
    push_receipt_field(&mut payload, source_owner_id.as_bytes());
    push_receipt_field(&mut payload, source_config_id.as_bytes());
    Ok(format!("govbind1:{:x}", Sha256::digest(payload)))
}

fn is_canonical_governance_text(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.chars().count() <= 2048
        && !value.chars().any(char::is_control)
}

impl PostTurnGovernanceExecutionBindingV1 {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Unbound => Ok(()),
            Self::Bound {
                binding_id,
                binding_revision,
            } if !binding_id.trim().is_empty()
                && binding_id.trim() == binding_id
                && *binding_revision > 0 =>
            {
                Ok(())
            }
            _ => Err(Error::invalid_input(
                "post_turn_governance_execution_binding",
                "execution binding identity or revision is invalid",
            )),
        }
    }

    pub const fn revision(&self) -> Option<u64> {
        match self {
            Self::Unbound => None,
            Self::Bound {
                binding_revision, ..
            } => Some(*binding_revision),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PostTurnGovernanceExecutionBlockReasonV1 {
    BindingUnavailable,
    CredentialMissing,
    CredentialLocked,
    CredentialRejected,
    ProviderPermissionDenied,
}

impl PostTurnGovernanceExecutionBlockReasonV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BindingUnavailable => "governance_execution_binding_unavailable",
            Self::CredentialMissing => "governance_credential_missing",
            Self::CredentialLocked => "governance_credential_locked",
            Self::CredentialRejected => "governance_credential_rejected",
            Self::ProviderPermissionDenied => "governance_provider_permission_denied",
        }
    }

    pub const fn blocked_status(self) -> PostTurnGovernanceJobStatusV2 {
        match self {
            Self::BindingUnavailable
            | Self::CredentialMissing
            | Self::CredentialLocked
            | Self::CredentialRejected => PostTurnGovernanceJobStatusV2::BlockedConfiguration,
            Self::ProviderPermissionDenied => PostTurnGovernanceJobStatusV2::BlockedPolicy,
        }
    }

    pub const fn is_credential_scoped(self) -> bool {
        matches!(
            self,
            Self::CredentialMissing | Self::CredentialLocked | Self::CredentialRejected
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostTurnGovernanceExecutionBlockAuthorityV1 {
    pub binding_id: Option<String>,
    pub binding_revision: Option<u64>,
    pub credential_ref_safe_id: Option<String>,
    pub typed_block_reason: PostTurnGovernanceExecutionBlockReasonV1,
    pub credential_generation: Option<u64>,
    pub provider_permission_generation: Option<u64>,
    pub blocked_at: u64,
}

impl PostTurnGovernanceExecutionBlockAuthorityV1 {
    pub fn binding_unavailable(blocked_at: u64) -> Self {
        Self {
            binding_id: None,
            binding_revision: None,
            credential_ref_safe_id: None,
            typed_block_reason: PostTurnGovernanceExecutionBlockReasonV1::BindingUnavailable,
            credential_generation: None,
            provider_permission_generation: None,
            blocked_at,
        }
    }

    pub fn exact_binding_block(
        execution_binding: &PostTurnGovernanceExecutionBindingV1,
        credential_ref_safe_id: Option<String>,
        typed_block_reason: PostTurnGovernanceExecutionBlockReasonV1,
        credential_generation: Option<u64>,
        provider_permission_generation: Option<u64>,
        blocked_at: u64,
    ) -> Result<Self> {
        let (binding_id, binding_revision) = match execution_binding {
            PostTurnGovernanceExecutionBindingV1::Unbound => (None, None),
            PostTurnGovernanceExecutionBindingV1::Bound {
                binding_id,
                binding_revision,
            } => (Some(binding_id.clone()), Some(*binding_revision)),
        };
        let authority = Self {
            binding_id,
            binding_revision,
            credential_ref_safe_id,
            typed_block_reason,
            credential_generation,
            provider_permission_generation,
            blocked_at,
        };
        authority.validate()?;
        Ok(authority)
    }

    pub fn validate(&self) -> Result<()> {
        if self.blocked_at == 0
            || self
                .binding_id
                .as_deref()
                .is_some_and(|value| value.trim().is_empty() || value.trim() != value)
            || self.binding_revision.is_some_and(|revision| revision == 0)
            || self
                .credential_ref_safe_id
                .as_deref()
                .is_some_and(|value| !is_sha256_digest(value))
            || self
                .credential_generation
                .is_some_and(|generation| generation == 0)
            || self
                .provider_permission_generation
                .is_some_and(|generation| generation == 0)
            || self.binding_id.is_some() != self.binding_revision.is_some()
        {
            return Err(Error::invalid_input(
                "post_turn_governance_execution_block_authority",
                "execution block authority is incomplete or invalid",
            ));
        }
        match self.typed_block_reason {
            PostTurnGovernanceExecutionBlockReasonV1::BindingUnavailable => {
                if self.credential_ref_safe_id.is_some()
                    || self.credential_generation.is_some()
                    || self.provider_permission_generation.is_some()
                {
                    return Err(Error::invalid_input(
                        "post_turn_governance_execution_block_authority",
                        "unbound authority must not invent binding or generation state",
                    ));
                }
            }
            reason @ (PostTurnGovernanceExecutionBlockReasonV1::CredentialMissing
            | PostTurnGovernanceExecutionBlockReasonV1::CredentialLocked
            | PostTurnGovernanceExecutionBlockReasonV1::CredentialRejected) => {
                if self.binding_id.is_none()
                    || self.credential_ref_safe_id.is_none()
                    || self.provider_permission_generation.is_some()
                    || (reason == PostTurnGovernanceExecutionBlockReasonV1::CredentialRejected
                        && self.credential_generation.is_none())
                {
                    return Err(Error::invalid_input(
                        "post_turn_governance_execution_block_authority",
                        "credential block requires exact binding and safe credential authority",
                    ));
                }
            }
            PostTurnGovernanceExecutionBlockReasonV1::ProviderPermissionDenied => {
                if self.binding_id.is_none()
                    || self.credential_generation.is_some()
                    || self.provider_permission_generation.is_none()
                {
                    return Err(Error::invalid_input(
                        "post_turn_governance_execution_block_authority",
                        "permission block requires exact binding and permission generation",
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostTurnGovernancePrivacyAuthorityV1 {
    pub policy_schema_version: u32,
    pub exact_policy_digest: String,
}

impl PostTurnGovernancePrivacyAuthorityV1 {
    pub fn validate(&self) -> Result<()> {
        if self.policy_schema_version == 0 || !is_sha256_digest(&self.exact_policy_digest) {
            return Err(Error::invalid_input(
                "post_turn_governance_privacy_authority",
                "privacy authority schema or digest is invalid",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostTurnGovernanceAttemptAuthorityV3 {
    pub binding_id: String,
    pub binding_revision: u64,
    pub model_id: String,
    pub privacy_authority: PostTurnGovernancePrivacyAuthorityV1,
    pub transcript_lifecycle_revision: u64,
    pub disclosure_authority_digest: String,
}

impl PostTurnGovernanceAttemptAuthorityV3 {
    pub fn validate(&self) -> Result<()> {
        if self.binding_id.trim().is_empty()
            || self.model_id.trim().is_empty()
            || self.binding_revision == 0
            || self.transcript_lifecycle_revision == 0
            || !is_sha256_digest(&self.disclosure_authority_digest)
        {
            return Err(Error::invalid_input(
                "post_turn_governance_attempt_authority",
                "attempt authority is incomplete or invalid",
            ));
        }
        self.privacy_authority.validate()?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PostTurnGovernanceDecisionDispositionV1 {
    Accepted,
    Rejected,
    Deferred,
    Mixed,
    NoCandidates,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostTurnGovernanceDecisionSummaryV1 {
    pub accepted_count: u32,
    pub rejected_count: u32,
    pub deferred_count: u32,
    pub disposition: PostTurnGovernanceDecisionDispositionV1,
    pub plane_count: u32,
    pub plane_digest: String,
}

impl PostTurnGovernanceDecisionSummaryV1 {
    pub fn from_semantic_report(report: &PostTurnSemanticGovernanceReport) -> Result<Self> {
        let accepted_count = u32::try_from(report.accepted_count).map_err(|_| {
            Error::invalid_input(
                "post_turn_governance_decision_summary",
                "accepted decision count exceeds the durable contract",
            )
        })?;
        let rejected_count = u32::try_from(report.rejected_count).map_err(|_| {
            Error::invalid_input(
                "post_turn_governance_decision_summary",
                "rejected decision count exceeds the durable contract",
            )
        })?;
        let deferred_count = u32::try_from(report.deferred_count).map_err(|_| {
            Error::invalid_input(
                "post_turn_governance_decision_summary",
                "deferred decision count exceeds the durable contract",
            )
        })?;
        let plane_count = u32::try_from(report.plane_reports.len()).map_err(|_| {
            Error::invalid_input(
                "post_turn_governance_decision_summary",
                "plane decision count exceeds the durable contract",
            )
        })?;
        let present = [accepted_count > 0, rejected_count > 0, deferred_count > 0]
            .into_iter()
            .filter(|value| *value)
            .count();
        let disposition = match present {
            0 => PostTurnGovernanceDecisionDispositionV1::NoCandidates,
            1 if accepted_count > 0 => PostTurnGovernanceDecisionDispositionV1::Accepted,
            1 if rejected_count > 0 => PostTurnGovernanceDecisionDispositionV1::Rejected,
            1 => PostTurnGovernanceDecisionDispositionV1::Deferred,
            _ => PostTurnGovernanceDecisionDispositionV1::Mixed,
        };
        let plane_payload = serde_json::to_vec(&report.plane_reports).map_err(|error| {
            Error::config("post_turn_governance_decision_summary", error.to_string())
        })?;
        let summary = Self {
            accepted_count,
            rejected_count,
            deferred_count,
            disposition,
            plane_count,
            plane_digest: format!("sha256:{:x}", Sha256::digest(plane_payload)),
        };
        summary.validate()?;
        Ok(summary)
    }

    pub fn validate(&self) -> Result<()> {
        let total = self
            .accepted_count
            .checked_add(self.rejected_count)
            .and_then(|value| value.checked_add(self.deferred_count))
            .ok_or_else(|| {
                Error::invalid_input(
                    "post_turn_governance_decision_summary",
                    "decision counts overflow",
                )
            })?;
        let disposition_matches = match self.disposition {
            PostTurnGovernanceDecisionDispositionV1::Accepted => {
                self.accepted_count > 0 && self.rejected_count == 0 && self.deferred_count == 0
            }
            PostTurnGovernanceDecisionDispositionV1::Rejected => {
                self.rejected_count > 0 && self.accepted_count == 0 && self.deferred_count == 0
            }
            PostTurnGovernanceDecisionDispositionV1::Deferred => {
                self.deferred_count > 0 && self.accepted_count == 0 && self.rejected_count == 0
            }
            PostTurnGovernanceDecisionDispositionV1::Mixed => {
                total > 0
                    && [
                        self.accepted_count > 0,
                        self.rejected_count > 0,
                        self.deferred_count > 0,
                    ]
                    .into_iter()
                    .filter(|present| *present)
                    .count()
                        > 1
            }
            PostTurnGovernanceDecisionDispositionV1::NoCandidates => total == 0,
        };
        if !disposition_matches || !is_sha256_digest(&self.plane_digest) {
            return Err(Error::invalid_input(
                "post_turn_governance_decision_summary",
                "decision disposition, counts, or plane digest is invalid",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostTurnGovernanceReceiptV3 {
    pub receipt_id: String,
    pub semantic_transaction_id: String,
    pub mutation_plan_digest: String,
    pub memory_post_image_digest: String,
    pub completed_at: u64,
    pub decision_summary: PostTurnGovernanceDecisionSummaryV1,
}

impl PostTurnGovernanceReceiptV3 {
    pub fn new(
        job_id: &str,
        semantic_transaction_id: impl Into<String>,
        mutation_plan_digest: impl Into<String>,
        memory_post_image_digest: impl Into<String>,
        completed_at: u64,
        decision_summary: PostTurnGovernanceDecisionSummaryV1,
    ) -> Result<Self> {
        let mut receipt = Self {
            receipt_id: String::new(),
            semantic_transaction_id: semantic_transaction_id.into(),
            mutation_plan_digest: mutation_plan_digest.into(),
            memory_post_image_digest: memory_post_image_digest.into(),
            completed_at,
            decision_summary,
        };
        receipt.receipt_id = receipt.canonical_receipt_id(job_id)?;
        Ok(receipt)
    }

    pub fn canonical_receipt_id(&self, job_id: &str) -> Result<String> {
        if job_id.trim().is_empty()
            || self.semantic_transaction_id.trim().is_empty()
            || !is_sha256_digest(&self.mutation_plan_digest)
            || !is_sha256_digest(&self.memory_post_image_digest)
            || self.completed_at == 0
        {
            return Err(Error::invalid_input(
                "post_turn_governance_receipt",
                "receipt identity payload is incomplete or invalid",
            ));
        }
        self.decision_summary.validate()?;
        let mut payload = b"bm-receipt-v3-id\0".to_vec();
        push_receipt_field(&mut payload, job_id.as_bytes());
        push_receipt_field(&mut payload, self.semantic_transaction_id.as_bytes());
        push_receipt_field(&mut payload, self.mutation_plan_digest.as_bytes());
        push_receipt_field(&mut payload, self.memory_post_image_digest.as_bytes());
        payload.extend_from_slice(&self.completed_at.to_be_bytes());
        payload.extend_from_slice(&self.decision_summary.accepted_count.to_be_bytes());
        payload.extend_from_slice(&self.decision_summary.rejected_count.to_be_bytes());
        payload.extend_from_slice(&self.decision_summary.deferred_count.to_be_bytes());
        payload.push(match self.decision_summary.disposition {
            PostTurnGovernanceDecisionDispositionV1::Accepted => 1,
            PostTurnGovernanceDecisionDispositionV1::Rejected => 2,
            PostTurnGovernanceDecisionDispositionV1::Deferred => 3,
            PostTurnGovernanceDecisionDispositionV1::Mixed => 4,
            PostTurnGovernanceDecisionDispositionV1::NoCandidates => 5,
        });
        payload.extend_from_slice(&self.decision_summary.plane_count.to_be_bytes());
        push_receipt_field(&mut payload, self.decision_summary.plane_digest.as_bytes());
        Ok(format!("sha256:{:x}", Sha256::digest(payload)))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostTurnGovernanceJobV3 {
    pub schema_version: u32,
    pub job_id: String,
    pub idempotency_key: String,
    pub scope_index_key: String,
    pub identity: PostTurnGovernanceIdentityV2,
    pub transcript_sequence: u64,
    pub transcript_digest: String,
    pub runtime_binding_digest: String,
    pub governance_contract_version: u32,
    pub execution_binding: PostTurnGovernanceExecutionBindingV1,
    pub privacy_authority: PostTurnGovernancePrivacyAuthorityV1,
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
    pub attempt_authority: Option<PostTurnGovernanceAttemptAuthorityV3>,
    pub execution_block_authority: Option<PostTurnGovernanceExecutionBlockAuthorityV1>,
    pub last_provider_permission_recovery_generation: Option<u64>,
    pub blocking_reason: Option<String>,
    pub last_error_class: Option<PostTurnGovernanceErrorClassV2>,
    pub receipt: Option<PostTurnGovernanceReceiptV3>,
    pub created_at: u64,
    pub updated_at: u64,
    pub terminal_at: Option<u64>,
}

impl PostTurnGovernanceJobV3 {
    #[allow(clippy::too_many_arguments)]
    pub fn pending(
        identity: PostTurnGovernanceIdentityV2,
        transcript_sequence: u64,
        transcript_digest: impl Into<String>,
        runtime_binding_digest: impl Into<String>,
        execution_binding: PostTurnGovernanceExecutionBindingV1,
        privacy_authority: PostTurnGovernancePrivacyAuthorityV1,
        candidate_ids: Vec<String>,
        tool_call_count: u32,
        max_attempts: u32,
        now_secs: u64,
    ) -> Result<Self> {
        let job_id = identity.job_id();
        let unbound = execution_binding == PostTurnGovernanceExecutionBindingV1::Unbound;
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
            execution_binding,
            privacy_authority,
            candidate_ids,
            tool_call_count,
            status: if unbound {
                PostTurnGovernanceJobStatusV2::BlockedConfiguration
            } else {
                PostTurnGovernanceJobStatusV2::Pending
            },
            state_revision: 1,
            attempt_count: 0,
            max_attempts,
            next_attempt_at: (!unbound).then_some(now_secs),
            lease_owner: None,
            lease_until: None,
            lease_epoch: 0,
            attempt_authority: None,
            execution_block_authority: unbound.then(|| {
                PostTurnGovernanceExecutionBlockAuthorityV1::binding_unavailable(now_secs)
            }),
            last_provider_permission_recovery_generation: None,
            blocking_reason: unbound.then(|| {
                PostTurnGovernanceExecutionBlockReasonV1::BindingUnavailable
                    .as_str()
                    .to_string()
            }),
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
        self.execution_binding.validate()?;
        self.privacy_authority.validate()?;
        if let Some(authority) = &self.execution_block_authority {
            authority.validate()?;
            if self.status != authority.typed_block_reason.blocked_status()
                || self.blocking_reason.as_deref() != Some(authority.typed_block_reason.as_str())
            {
                return Err(Error::invalid_input(
                    "post_turn_governance_job",
                    "execution block authority differs from blocked job state",
                ));
            }
        } else if matches!(
            self.status,
            PostTurnGovernanceJobStatusV2::BlockedConfiguration
                | PostTurnGovernanceJobStatusV2::BlockedPolicy
        ) {
            return Err(Error::invalid_input(
                "post_turn_governance_job",
                "configuration or policy block requires execution block authority",
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
            || self
                .last_provider_permission_recovery_generation
                .is_some_and(|generation| generation == 0)
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
            match &self.execution_binding {
                PostTurnGovernanceExecutionBindingV1::Bound {
                    binding_id,
                    binding_revision,
                } if binding_id == &authority.binding_id
                    && binding_revision == &authority.binding_revision => {}
                _ => {
                    return Err(Error::invalid_input(
                        "post_turn_governance_job",
                        "attempt authority differs from the bound execution authority",
                    ));
                }
            }
            if authority.privacy_authority != self.privacy_authority {
                return Err(Error::invalid_input(
                    "post_turn_governance_job",
                    "attempt privacy authority differs from the pinned policy",
                ));
            }
        }
        if self.attempt_authority.is_some()
            && !matches!(
                self.execution_binding,
                PostTurnGovernanceExecutionBindingV1::Bound { .. }
            )
        {
            return Err(Error::invalid_input(
                "post_turn_governance_job",
                "attempt authority requires one bound execution authority",
            ));
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
            if !is_sha256_digest(&receipt.receipt_id)
                || receipt.semantic_transaction_id.trim().is_empty()
                || !is_sha256_digest(&receipt.mutation_plan_digest)
                || !is_sha256_digest(&receipt.memory_post_image_digest)
                || receipt.completed_at == 0
            {
                return Err(Error::invalid_input(
                    "post_turn_governance_job",
                    "completion receipt is invalid",
                ));
            }
            receipt.decision_summary.validate()?;
            if receipt.receipt_id != receipt.canonical_receipt_id(&self.job_id)? {
                return Err(Error::invalid_input(
                    "post_turn_governance_job",
                    "completion receipt identity differs from its canonical payload",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostTurnGovernanceJobRefV2 {
    pub job_schema_version: u32,
    pub job_id: String,
    pub job_key: String,
    pub state_revision: u64,
    pub status: PostTurnGovernanceJobStatusV2,
    pub next_attempt_at: Option<u64>,
    pub lease_until: Option<u64>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl PostTurnGovernanceJobRefV2 {
    pub fn from_job(job: &PostTurnGovernanceJobV3) -> Self {
        Self {
            job_schema_version: POST_TURN_GOVERNANCE_JOB_SCHEMA_VERSION,
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
pub struct PostTurnGovernanceScopeIndexV3 {
    pub schema_version: u32,
    pub scope_index_key: String,
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub channel_id: String,
    pub chat_id: String,
    pub index_revision: u64,
    pub active_jobs: Vec<PostTurnGovernanceJobRefV2>,
    pub recent_terminal_jobs: Vec<PostTurnGovernanceJobRefV2>,
    pub reconciliation_cursors: Vec<PostTurnGovernanceReconciliationCursorV1>,
    pub updated_at: u64,
}

impl PostTurnGovernanceScopeIndexV3 {
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
                job.job_schema_version != POST_TURN_GOVERNANCE_JOB_SCHEMA_VERSION
                    || !job.status.is_active()
                    || job.job_id != job.job_key
                    || job.state_revision == 0
                    || job.updated_at < job.created_at
            })
            || self.recent_terminal_jobs.iter().any(|job| {
                job.job_schema_version != POST_TURN_GOVERNANCE_JOB_SCHEMA_VERSION
                    || !job.status.is_terminal()
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

fn push_receipt_field(payload: &mut Vec<u8>, field: &[u8]) {
    payload.extend_from_slice(&(field.len() as u64).to_be_bytes());
    payload.extend_from_slice(field);
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
