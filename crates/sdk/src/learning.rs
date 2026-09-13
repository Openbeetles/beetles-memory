use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use bm_core::llm::{LlmClient, LlmHttpClient};
use bm_core::memory::{
    PostTurnGovernanceErrorClassV2, PostTurnGovernanceJobStatusV2, ProceduralFeedbackErrorClassV1,
    ProceduralFeedbackJobStatusV1, ProceduralFeedbackJobV2, ProceduralFeedbackReceiptV2,
};
use bm_core::{Error, Result};

use crate::store_internal::procedural_feedback::ProceduralFeedbackCompletionOutcome;
use crate::{
    MemoryGovernanceActiveJobsRequest, MemoryGovernanceAttemptAuthorityRequest,
    MemoryGovernanceBlockKind, MemoryGovernanceClaimedJobBlockRequest,
    MemoryGovernanceJobBlockRequest, MemoryGovernanceJobClaimRequest,
    MemoryGovernanceJobFailRequest, MemoryGovernanceJobResumeRequest,
    MemoryGovernanceJobRetryRequest, MemoryGovernanceJobRunReport, MemoryGovernanceJobRunRequest,
    MemoryRuntime, PostTurnGovernanceJobV3,
};

const MAX_LEARNING_CYCLE_JOBS: usize = 32;
const MAX_LEARNING_LEASE_SECS: u64 = 15 * 60;

/// Management intent, accepted only from the mounted runtime's active governing
/// SystemGovernor. It never grants reporting authority merely by deserialization.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryProceduralProducerControlRequest {
    pub operation_id: String,
    pub spec: bm_core::memory::ProceduralProducerSpecV1,
    pub state: bm_core::memory::ProceduralProducerStateV1,
    pub expected_revision: Option<bm_core::memory::ProceduralProducerRevisionRefV1>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct MemoryProceduralProducerControlReport {
    pub binding: bm_core::memory::ProceduralProducerBindingV1,
    pub receipt: bm_core::memory::MemoryMutationReceipt,
}

/// Explicit capacity recovery, accepted only from the governing SystemGovernor.
/// Wake or runtime reopen never imply permission to remove a durable block.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryProceduralReconciliationResumeRequest {
    pub operation_id: String,
    pub job_id: String,
    pub expected_state_revision: u64,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct MemoryProceduralReconciliationResumeReport {
    pub job: ProceduralFeedbackJobV2,
    pub receipt: bm_core::memory::MemoryMutationReceipt,
    pub replayed: bool,
}

/// Read-only discovery for the exact mounted subject; not recovery authority.
#[derive(Clone, Debug)]
pub struct MemoryProceduralReconciliationStatusRequest {
    pub authority: MemoryLearningAttachmentStatusAuthority,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct MemoryProceduralReconciliationRecoveryTarget {
    pub job_id: String,
    pub expected_state_revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct MemoryProceduralReconciliationStatusReport {
    pub read_availability: bm_core::memory::ProceduralLearningReadAvailabilityV1,
    pub block_reason: Option<bm_core::memory::ProceduralReconciliationBlockV1>,
    pub recovery: Option<MemoryProceduralReconciliationRecoveryTarget>,
}

/// An in-process reporting capability minted by the active governing runtime.
/// It is deliberately neither serializable nor caller-constructible. Persistent
/// producer metadata alone cannot be converted into submission authority.
#[derive(Clone)]
pub struct MemoryProceduralSubmissionCapability {
    pub(crate) identity: MemoryLearningAttachmentIdentity,
    pub(crate) store_incarnation: String,
    pub(crate) binding: bm_core::memory::ProceduralProducerBindingV1,
}

impl MemoryProceduralSubmissionCapability {
    /// Identity metadata for trusted ingress matching, not a constructor or a
    /// substitute for the SDK's current Store/registry/revision checks.
    pub fn principal(&self) -> &bm_core::memory::ProceduralProducerPrincipalV1 {
        &self.binding.spec.principal
    }
}

/// Non-wire proof for exactly one admitted canonical turn. Kept out of the
/// persistent record and out of every public Store mutation constructor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProceduralIntakeAuthorization {
    pub(crate) store_authority_digest: String,
    pub(crate) store_incarnation: String,
    pub(crate) current_head: bm_core::memory::ProceduralProducerHeadV1,
    pub(crate) evidence: bm_core::memory::PostTurnLearningEvidenceV2,
    pub(crate) source_preconditions: Vec<crate::StoreJsonPrecondition>,
}

impl ProceduralIntakeAuthorization {
    pub(crate) fn authorizes(&self, record: &bm_core::memory::TranscriptTurnRecord) -> bool {
        record.learning_evidence.as_ref() == Some(&self.evidence)
            && record.canonical_turn_digest == self.evidence.canonical_turn_digest
            && record.key.memory_space_id == self.evidence.memory_space_id
            && record.subject == self.evidence.mounted_subject_id
            && record.key.conversation_id == self.evidence.conversation_id
            && record.turn_id == self.evidence.turn_id
            && record.key.channel_id == self.current_head.scope.channel_id
    }
}

pub trait MemoryLearningWakeSink: Send + Sync {
    fn wake(&self);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryLearningAttachmentIdentity {
    store_authority_digest: String,
    registry_digest: String,
    memory_space_id: String,
    mounted_subject_id: String,
    channel_id: String,
    chat_id: String,
}

impl MemoryLearningAttachmentIdentity {
    pub(crate) fn new(
        store_authority_digest: String,
        registry_digest: String,
        memory_space_id: String,
        mounted_subject_id: String,
        channel_id: String,
        chat_id: String,
    ) -> Self {
        Self {
            store_authority_digest,
            registry_digest,
            memory_space_id,
            mounted_subject_id,
            channel_id,
            chat_id,
        }
    }

    pub fn shares_store_and_registry_with(&self, other: &Self) -> bool {
        self.store_authority_digest == other.store_authority_digest
            && self.registry_digest == other.registry_digest
            && self.memory_space_id == other.memory_space_id
    }

    pub fn mounted_subject_id(&self) -> &str {
        &self.mounted_subject_id
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryLearningServiceStatusAuthority {
    identity: MemoryLearningAttachmentIdentity,
}

impl MemoryLearningServiceStatusAuthority {
    pub(crate) fn new(identity: MemoryLearningAttachmentIdentity) -> Self {
        Self { identity }
    }

    pub fn authorizes(&self, identity: &MemoryLearningAttachmentIdentity) -> bool {
        self.identity.shares_store_and_registry_with(identity)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryLearningServiceControlOperation {
    CredentialRecovery,
    ProviderPermissionRecovery,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryLearningServiceControlAuthority {
    identity: MemoryLearningAttachmentIdentity,
    operation: MemoryLearningServiceControlOperation,
    system_governor_subject_id: String,
}

impl MemoryLearningServiceControlAuthority {
    pub(crate) fn new(
        identity: MemoryLearningAttachmentIdentity,
        operation: MemoryLearningServiceControlOperation,
        system_governor_subject_id: String,
    ) -> Self {
        Self {
            identity,
            operation,
            system_governor_subject_id,
        }
    }

    pub fn authorizes(
        &self,
        identity: &MemoryLearningAttachmentIdentity,
        operation: MemoryLearningServiceControlOperation,
    ) -> bool {
        self.identity == *identity && self.operation == operation
    }

    pub(crate) fn system_governor_subject_id(&self) -> &str {
        &self.system_governor_subject_id
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryLearningServiceControlAuthorities {
    credential_recovery: MemoryLearningServiceControlAuthority,
    provider_permission_recovery: MemoryLearningServiceControlAuthority,
}

impl MemoryLearningServiceControlAuthorities {
    pub(crate) fn new(
        credential_recovery: MemoryLearningServiceControlAuthority,
        provider_permission_recovery: MemoryLearningServiceControlAuthority,
    ) -> Self {
        Self {
            credential_recovery,
            provider_permission_recovery,
        }
    }

    pub fn credential_recovery(&self) -> MemoryLearningServiceControlAuthority {
        self.credential_recovery.clone()
    }

    pub fn provider_permission_recovery(&self) -> MemoryLearningServiceControlAuthority {
        self.provider_permission_recovery.clone()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryLearningAttachmentStatusAuthority {
    identity: MemoryLearningAttachmentIdentity,
}

impl MemoryLearningAttachmentStatusAuthority {
    pub(crate) fn new(identity: MemoryLearningAttachmentIdentity) -> Self {
        Self { identity }
    }

    pub fn authorizes(&self, identity: &MemoryLearningAttachmentIdentity) -> bool {
        self.identity == *identity
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImmutableGovernanceExecutionBinding {
    pub binding_id: String,
    pub binding_revision: u64,
    pub protocol: crate::PostTurnGovernanceProviderProtocolV1,
    pub endpoint: String,
    pub model_id: String,
    pub credential_reference: Option<String>,
    pub request_timeout_ms: u64,
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub provider_permission_generation: u64,
    pub canonical_digest: String,
}

impl ImmutableGovernanceExecutionBinding {
    fn validate(&self) -> Result<()> {
        if self.binding_id.trim().is_empty()
            || self.binding_id.trim() != self.binding_id
            || self.binding_revision == 0
            || self.endpoint.trim().is_empty()
            || self.endpoint.trim() != self.endpoint
            || self.model_id.trim().is_empty()
            || self.model_id.trim() != self.model_id
            || self
                .credential_reference
                .as_deref()
                .is_some_and(|value| value.trim().is_empty() || value.trim() != value)
            || self.request_timeout_ms == 0
            || self.max_input_tokens == 0
            || self.max_output_tokens == 0
            || self.provider_permission_generation == 0
            || self.canonical_digest.len() != 71
            || !self.canonical_digest.starts_with("sha256:")
            || !self.canonical_digest[7..]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(Error::invalid_input(
                "memory_learning_binding",
                "immutable execution binding is incomplete",
            ));
        }
        Ok(())
    }

    fn from_snapshot(snapshot: crate::PostTurnGovernanceBindingSnapshotV1) -> Self {
        Self {
            binding_id: snapshot.binding_id,
            binding_revision: snapshot.binding_revision,
            protocol: snapshot.protocol,
            endpoint: snapshot.endpoint,
            model_id: snapshot.model_id,
            credential_reference: snapshot.credential_reference,
            request_timeout_ms: snapshot.request_timeout_ms,
            max_input_tokens: snapshot.max_input_tokens,
            max_output_tokens: snapshot.max_output_tokens,
            provider_permission_generation: snapshot.provider_permission_generation,
            canonical_digest: snapshot.canonical_digest,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryLearningCycleRequest {
    pub lease_owner: String,
    pub lease_duration_secs: u64,
}

impl MemoryLearningCycleRequest {
    fn validate(&self) -> Result<()> {
        if self.lease_owner.trim().is_empty()
            || self.lease_owner.trim() != self.lease_owner
            || self.lease_duration_secs == 0
            || self.lease_duration_secs > MAX_LEARNING_LEASE_SECS
        {
            return Err(Error::invalid_input(
                "memory_learning_cycle",
                "lease owner and bounded lease duration must be exact",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorizedGovernanceEnvelope {
    pub job_id: String,
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub conversation_id: String,
    pub transcript_digest: String,
    pub transcript_lifecycle_revision: u64,
    pub privacy_authority_digest: String,
    pub binding_id: String,
    pub binding_revision: u64,
    pub model_id: String,
    pub candidate_count: usize,
    pub tool_call_count: u32,
}

impl AuthorizedGovernanceEnvelope {
    fn from_claimed(job: &PostTurnGovernanceJobV3) -> Result<Self> {
        let authority = job.attempt_authority.as_ref().ok_or_else(|| {
            Error::config(
                "memory_learning_envelope",
                "claimed job is missing immutable attempt authority",
            )
        })?;
        Ok(Self {
            job_id: job.job_id.clone(),
            memory_space_id: job.identity.memory_space_id.clone(),
            mounted_subject_id: job.identity.mounted_subject_id.clone(),
            conversation_id: job.identity.conversation_id.clone(),
            transcript_digest: job.transcript_digest.clone(),
            transcript_lifecycle_revision: authority.transcript_lifecycle_revision,
            privacy_authority_digest: authority.privacy_authority.exact_policy_digest.clone(),
            binding_id: authority.binding_id.clone(),
            binding_revision: authority.binding_revision,
            model_id: authority.model_id.clone(),
            candidate_count: job.candidate_ids.len(),
            tool_call_count: job.tool_call_count,
        })
    }
}

pub struct GovernanceEgressAuthority {
    runtime: Arc<MemoryRuntime>,
    job_id: String,
    lease_owner: String,
    lease_epoch: u64,
}

impl GovernanceEgressAuthority {
    pub fn revalidate_before_egress(&self) -> Result<()> {
        self.runtime.revalidate_governance_egress_authority(
            &self.job_id,
            &self.lease_owner,
            self.lease_epoch,
        )
    }
}

pub trait GovernanceExecutionOperation {
    fn run(
        &mut self,
        http: &mut dyn LlmHttpClient,
        llm: &(dyn LlmClient + Send + Sync),
    ) -> Result<()>;
}

pub trait GovernanceExecutionPort: Send {
    fn execute(
        &mut self,
        envelope: &AuthorizedGovernanceEnvelope,
        binding: &ImmutableGovernanceExecutionBinding,
        egress: &GovernanceEgressAuthority,
        operation: &mut dyn GovernanceExecutionOperation,
    ) -> std::result::Result<(), GovernanceExecutionPortFailure>;
}

pub enum GovernanceExecutionPortFailure {
    CapabilityUnavailable,
    CredentialMissing {
        credential_ref_safe_id: String,
    },
    CredentialLocked {
        credential_ref_safe_id: String,
    },
    CredentialRejected {
        credential_ref_safe_id: String,
        credential_generation: u64,
    },
    ProviderPermissionDenied {
        provider_permission_generation: u64,
    },
    Other(Error),
}

impl From<Error> for GovernanceExecutionPortFailure {
    fn from(error: Error) -> Self {
        Self::Other(error)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryLearningStateReport {
    pub job: PostTurnGovernanceJobV3,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryProceduralLearningStateReport {
    pub job: ProceduralFeedbackJobV2,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryProceduralLearningRunReport {
    pub job: ProceduralFeedbackJobV2,
    pub receipt: ProceduralFeedbackReceiptV2,
    pub replayed: bool,
}

/// A durable page in the same procedural lane. Progress is not a retry or a
/// completed feedback application, and has no invented turn/count receipt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryProceduralReconciliationRunReport {
    pub job: ProceduralFeedbackJobV2,
    pub receipt: bm_core::memory::MemoryMutationReceipt,
    pub completed: bool,
    pub replayed: bool,
}

#[derive(Debug)]
pub enum MemoryLearningCycleOutcome {
    Idle { reason: String },
    Completed(MemoryGovernanceJobRunReport),
    Retrying(MemoryLearningStateReport),
    Blocked(MemoryLearningStateReport),
    Cancelled(MemoryLearningStateReport),
    Failed(MemoryLearningStateReport),
    ProceduralCompleted(MemoryProceduralLearningRunReport),
    ProceduralReconciliation(MemoryProceduralReconciliationRunReport),
    ProceduralRetrying(MemoryProceduralLearningStateReport),
    ProceduralBlocked(MemoryProceduralLearningStateReport),
    ProceduralFailed(MemoryProceduralLearningStateReport),
}

#[derive(Clone)]
pub struct MemoryLearningEngine {
    runtime: Arc<MemoryRuntime>,
    next_lane: Arc<AtomicU8>,
}

impl MemoryLearningEngine {
    pub fn attach(runtime: Arc<MemoryRuntime>) -> Result<Self> {
        runtime.active_governance_jobs(MemoryGovernanceActiveJobsRequest { limit: 1 })?;
        runtime.due_procedural_feedback_jobs(1)?;
        Ok(Self {
            runtime,
            next_lane: Arc::new(AtomicU8::new(0)),
        })
    }

    pub fn runtime(&self) -> &Arc<MemoryRuntime> {
        &self.runtime
    }

    pub fn attachment_identity(&self) -> Result<MemoryLearningAttachmentIdentity> {
        self.runtime.governance_attachment_identity()
    }

    pub fn run_due_cycle(
        &self,
        request: MemoryLearningCycleRequest,
        port: &mut dyn GovernanceExecutionPort,
    ) -> Result<MemoryLearningCycleOutcome> {
        request.validate()?;
        let now_secs = self.runtime.governance_now_secs();
        let jobs = self
            .runtime
            .active_governance_jobs(MemoryGovernanceActiveJobsRequest {
                limit: MAX_LEARNING_CYCLE_JOBS,
            })?
            .jobs;
        let semantic_job = jobs.into_iter().find(|job| is_due(job, now_secs));
        let procedural_job = self
            .runtime
            .due_procedural_feedback_jobs(MAX_LEARNING_CYCLE_JOBS)?
            .into_iter()
            .next();
        let prefer_procedural = self.next_lane.fetch_xor(1, Ordering::AcqRel) == 0;
        if let Some(job) = procedural_job.as_ref() {
            if prefer_procedural || semantic_job.is_none() {
                return self.run_procedural_cycle(job.clone(), &request, now_secs);
            }
        }
        let Some(mut job) = semantic_job else {
            if let Some(job) = procedural_job {
                return self.run_procedural_cycle(job, &request, now_secs);
            }
            return Ok(MemoryLearningCycleOutcome::Idle {
                reason: "no_due_learning_job".to_string(),
            });
        };

        if job.status == PostTurnGovernanceJobStatusV2::BlockedPolicy {
            return Ok(MemoryLearningCycleOutcome::Blocked(
                MemoryLearningStateReport {
                    job,
                    reason: "governance_policy_requires_explicit_resume".to_string(),
                },
            ));
        }
        if job.status == PostTurnGovernanceJobStatusV2::BlockedConfiguration
            && job.execution_block_authority.as_ref().is_some_and(|block| {
                block.typed_block_reason
                    != crate::PostTurnGovernanceExecutionBlockReasonV1::BindingUnavailable
            })
        {
            return Ok(MemoryLearningCycleOutcome::Blocked(
                MemoryLearningStateReport {
                    job,
                    reason: "governance_configuration_requires_authorized_resume".to_string(),
                },
            ));
        }
        if job.status == PostTurnGovernanceJobStatusV2::BlockedCapability {
            job = self
                .runtime
                .resume_governance_job(MemoryGovernanceJobResumeRequest {
                    job_id: job.job_id.clone(),
                })?
                .job;
        }
        let binding = match self.runtime.governance_binding_snapshot_for_job(&job) {
            Ok(snapshot) => ImmutableGovernanceExecutionBinding::from_snapshot(snapshot),
            Err(_) => {
                if job.status == PostTurnGovernanceJobStatusV2::BlockedConfiguration {
                    return Ok(MemoryLearningCycleOutcome::Blocked(
                        MemoryLearningStateReport {
                            job,
                            reason: "governance_execution_binding_unavailable".to_string(),
                        },
                    ));
                }
                let blocked = self
                    .runtime
                    .block_governance_job(MemoryGovernanceJobBlockRequest {
                        job_id: job.job_id,
                        kind: MemoryGovernanceBlockKind::Configuration,
                        reason: "governance_execution_binding_unavailable".to_string(),
                        execution_block_authority: Some(
                            crate::PostTurnGovernanceExecutionBlockAuthorityV1::exact_binding_block(
                                &job.execution_binding,
                                None,
                                crate::PostTurnGovernanceExecutionBlockReasonV1::BindingUnavailable,
                                None,
                                None,
                                now_secs,
                            )?,
                        ),
                    })?
                    .job;
                return Ok(MemoryLearningCycleOutcome::Blocked(
                    MemoryLearningStateReport {
                        job: blocked,
                        reason: "governance_execution_binding_unavailable".to_string(),
                    },
                ));
            }
        };
        binding.validate()?;
        if matches!(
            &job.execution_binding,
            crate::PostTurnGovernanceExecutionBindingV1::Bound {
                binding_id,
                binding_revision,
            } if binding_id != &binding.binding_id || binding_revision != &binding.binding_revision
        ) {
            let blocked = self
                .runtime
                .block_governance_job(MemoryGovernanceJobBlockRequest {
                    job_id: job.job_id,
                    kind: MemoryGovernanceBlockKind::Configuration,
                    reason: "governance_execution_binding_revision_unavailable".to_string(),
                    execution_block_authority: Some(
                        crate::PostTurnGovernanceExecutionBlockAuthorityV1::exact_binding_block(
                            &job.execution_binding,
                            None,
                            crate::PostTurnGovernanceExecutionBlockReasonV1::BindingUnavailable,
                            None,
                            None,
                            now_secs,
                        )?,
                    ),
                })?
                .job;
            return Ok(MemoryLearningCycleOutcome::Blocked(
                MemoryLearningStateReport {
                    job: blocked,
                    reason: "governance_execution_binding_revision_unavailable".to_string(),
                },
            ));
        }

        let authority = match self.runtime.prepare_governance_attempt_authority(
            MemoryGovernanceAttemptAuthorityRequest {
                job_id: job.job_id.clone(),
                binding_id: binding.binding_id.clone(),
                binding_revision: binding.binding_revision,
                model_id: binding.model_id.clone(),
            },
        ) {
            Ok(report) => report.authority,
            Err(_) => {
                let blocked = self
                    .runtime
                    .block_governance_job(MemoryGovernanceJobBlockRequest {
                        job_id: job.job_id,
                        kind: MemoryGovernanceBlockKind::Policy,
                        reason: "governance_disclosure_authority_unavailable".to_string(),
                        execution_block_authority: None,
                    })?
                    .job;
                return Ok(MemoryLearningCycleOutcome::Blocked(
                    MemoryLearningStateReport {
                        job: blocked,
                        reason: "governance_disclosure_authority_unavailable".to_string(),
                    },
                ));
            }
        };
        let lease_until = now_secs
            .checked_add(request.lease_duration_secs)
            .ok_or_else(|| Error::config("memory_learning_cycle", "lease deadline overflow"))?;
        let claimed = match self
            .runtime
            .claim_governance_job(MemoryGovernanceJobClaimRequest {
                job_id: job.job_id,
                lease_owner: request.lease_owner.clone(),
                lease_until,
                authority,
            }) {
            Ok(report) => report.job,
            Err(Error::Conflict { .. }) => {
                return Ok(MemoryLearningCycleOutcome::Idle {
                    reason: "governance_claim_lost".to_string(),
                });
            }
            Err(error) => return Err(error),
        };
        let envelope = AuthorizedGovernanceEnvelope::from_claimed(&claimed)?;
        let egress = GovernanceEgressAuthority {
            runtime: Arc::clone(&self.runtime),
            job_id: claimed.job_id.clone(),
            lease_owner: request.lease_owner.clone(),
            lease_epoch: claimed.lease_epoch,
        };
        let mut operation = RuntimeGovernanceExecutionOperation {
            runtime: Arc::clone(&self.runtime),
            request: Some(MemoryGovernanceJobRunRequest {
                job_id: claimed.job_id.clone(),
                lease_owner: request.lease_owner.clone(),
                lease_epoch: claimed.lease_epoch,
            }),
            report: None,
        };
        let execution_result = port.execute(&envelope, &binding, &egress, &mut operation);
        if let Some(report) = operation.report.take() {
            return Ok(MemoryLearningCycleOutcome::Completed(report));
        }
        match execution_result {
            Ok(()) => self.transition_classified_failure(
                claimed,
                &request,
                PostTurnGovernanceErrorClassV2::SchemaViolation,
            ),
            Err(failure) => self.transition_execution_failure(claimed, &request, &egress, failure),
        }
    }

    fn run_procedural_cycle(
        &self,
        job: ProceduralFeedbackJobV2,
        request: &MemoryLearningCycleRequest,
        now_secs: u64,
    ) -> Result<MemoryLearningCycleOutcome> {
        let lease_until = now_secs
            .checked_add(request.lease_duration_secs)
            .ok_or_else(|| Error::config("memory_learning_cycle", "lease deadline overflow"))?;
        let claimed = match self.runtime.claim_due_procedural_feedback_job(
            &job.job_id,
            &request.lease_owner,
            lease_until,
        ) {
            Ok(job) => job,
            Err(Error::Conflict { .. }) => {
                return Ok(MemoryLearningCycleOutcome::Idle {
                    reason: "procedural_claim_lost".to_string(),
                });
            }
            Err(error) => return Err(error),
        };
        let result = if matches!(
            claimed.work,
            bm_core::memory::ProceduralLearningWorkV1::Reconcile { .. }
        ) {
            match self
                .runtime
                .run_claimed_procedural_reconciliation_job(&claimed, &request.lease_owner)
            {
                Ok(page) => {
                    return Ok(MemoryLearningCycleOutcome::ProceduralReconciliation(
                        MemoryProceduralReconciliationRunReport {
                            job: page.job,
                            receipt: page.receipt,
                            completed: page.completed,
                            replayed: page.replayed,
                        },
                    ))
                }
                Err(error) => Err(error),
            }
        } else {
            self.runtime
                .run_claimed_procedural_feedback_job(&claimed, &request.lease_owner)
        };
        match result {
            Ok(ProceduralFeedbackCompletionOutcome::Committed { job, receipt }) => {
                Ok(MemoryLearningCycleOutcome::ProceduralCompleted(
                    MemoryProceduralLearningRunReport {
                        job,
                        receipt,
                        replayed: false,
                    },
                ))
            }
            Ok(ProceduralFeedbackCompletionOutcome::Replayed { job, receipt }) => {
                Ok(MemoryLearningCycleOutcome::ProceduralCompleted(
                    MemoryProceduralLearningRunReport {
                        job,
                        receipt,
                        replayed: true,
                    },
                ))
            }
            Err(error) => {
                let (error_class, repair_required) = classify_procedural_error(&error);
                if repair_required || error_class == ProceduralFeedbackErrorClassV1::AuthorityDenied
                {
                    let failed = self.runtime.terminate_claimed_procedural_feedback_job(
                        &claimed,
                        &request.lease_owner,
                        error_class,
                    )?;
                    return Ok(MemoryLearningCycleOutcome::ProceduralFailed(
                        MemoryProceduralLearningStateReport {
                            job: failed,
                            reason: if repair_required {
                                "procedural_feedback_repair_required"
                            } else {
                                "procedural_feedback_authority_denied"
                            }
                            .to_string(),
                        },
                    ));
                }
                let retrying = self.runtime.retry_claimed_procedural_feedback_job(
                    &claimed,
                    &request.lease_owner,
                    error_class,
                )?;
                if retrying.status == ProceduralFeedbackJobStatusV1::BlockedCapacity {
                    Ok(MemoryLearningCycleOutcome::ProceduralBlocked(
                        MemoryProceduralLearningStateReport {
                            job: retrying,
                            reason: "procedural_reconciliation_capacity_requires_explicit_resume"
                                .into(),
                        },
                    ))
                } else if retrying.status == ProceduralFeedbackJobStatusV1::DeadLetter {
                    Ok(MemoryLearningCycleOutcome::ProceduralFailed(
                        MemoryProceduralLearningStateReport {
                            job: retrying,
                            reason: "procedural_retry_attempts_exhausted".to_string(),
                        },
                    ))
                } else {
                    Ok(MemoryLearningCycleOutcome::ProceduralRetrying(
                        MemoryProceduralLearningStateReport {
                            job: retrying,
                            reason: format!("procedural_{error_class:?}"),
                        },
                    ))
                }
            }
        }
    }

    fn transition_execution_failure(
        &self,
        claimed: PostTurnGovernanceJobV3,
        request: &MemoryLearningCycleRequest,
        egress: &GovernanceEgressAuthority,
        failure: GovernanceExecutionPortFailure,
    ) -> Result<MemoryLearningCycleOutcome> {
        if egress.revalidate_before_egress().is_err() {
            let current = self
                .runtime
                .governance_job_status(crate::MemoryGovernanceJobStatusRequest {
                    job_id: claimed.job_id.clone(),
                })?
                .job;
            if current.status == PostTurnGovernanceJobStatusV2::Cancelled {
                return Ok(MemoryLearningCycleOutcome::Cancelled(
                    MemoryLearningStateReport {
                        job: current,
                        reason: "governance_source_authority_revoked".to_string(),
                    },
                ));
            }
            let blocked = self
                .runtime
                .block_claimed_governance_job(MemoryGovernanceClaimedJobBlockRequest {
                    job_id: claimed.job_id,
                    lease_owner: request.lease_owner.clone(),
                    lease_epoch: claimed.lease_epoch,
                    kind: MemoryGovernanceBlockKind::Policy,
                    reason: "governance_egress_authority_revoked".to_string(),
                    execution_block_authority: None,
                })?
                .job;
            return Ok(MemoryLearningCycleOutcome::Blocked(
                MemoryLearningStateReport {
                    job: blocked,
                    reason: "governance_egress_authority_revoked".to_string(),
                },
            ));
        }
        let error = match failure {
            GovernanceExecutionPortFailure::CapabilityUnavailable => {
                let blocked = self
                    .runtime
                    .block_claimed_governance_job(MemoryGovernanceClaimedJobBlockRequest {
                        job_id: claimed.job_id,
                        lease_owner: request.lease_owner.clone(),
                        lease_epoch: claimed.lease_epoch,
                        kind: MemoryGovernanceBlockKind::Capability,
                        reason: "governance_execution_capability_unavailable".to_string(),
                        execution_block_authority: None,
                    })?
                    .job;
                return Ok(MemoryLearningCycleOutcome::Blocked(
                    MemoryLearningStateReport {
                        job: blocked,
                        reason: "governance_execution_capability_unavailable".to_string(),
                    },
                ));
            }
            GovernanceExecutionPortFailure::CredentialMissing {
                credential_ref_safe_id,
            } => {
                return self.block_claimed_execution(
                    claimed,
                    request,
                    MemoryGovernanceBlockKind::Configuration,
                    crate::PostTurnGovernanceExecutionBlockReasonV1::CredentialMissing,
                    Some(credential_ref_safe_id),
                    None,
                    None,
                );
            }
            GovernanceExecutionPortFailure::CredentialLocked {
                credential_ref_safe_id,
            } => {
                return self.block_claimed_execution(
                    claimed,
                    request,
                    MemoryGovernanceBlockKind::Configuration,
                    crate::PostTurnGovernanceExecutionBlockReasonV1::CredentialLocked,
                    Some(credential_ref_safe_id),
                    None,
                    None,
                );
            }
            GovernanceExecutionPortFailure::CredentialRejected {
                credential_ref_safe_id,
                credential_generation,
            } => {
                return self.block_claimed_execution(
                    claimed,
                    request,
                    MemoryGovernanceBlockKind::Configuration,
                    crate::PostTurnGovernanceExecutionBlockReasonV1::CredentialRejected,
                    Some(credential_ref_safe_id),
                    Some(credential_generation),
                    None,
                );
            }
            GovernanceExecutionPortFailure::ProviderPermissionDenied {
                provider_permission_generation,
            } => {
                let effective_generation = claimed
                    .last_provider_permission_recovery_generation
                    .unwrap_or(provider_permission_generation)
                    .max(provider_permission_generation);
                return self.block_claimed_execution(
                    claimed,
                    request,
                    MemoryGovernanceBlockKind::Policy,
                    crate::PostTurnGovernanceExecutionBlockReasonV1::ProviderPermissionDenied,
                    None,
                    None,
                    Some(effective_generation),
                );
            }
            GovernanceExecutionPortFailure::Other(error) => error,
        };
        if error.class() == Some(bm_core::ErrorClass::Conflict)
            && matches!(
                error.stage(),
                "memory_write_transaction_precondition_failed"
                    | "subject_soul_store_expected_state"
            )
        {
            return Err(error);
        }
        if let Error::Http { status_code, .. } = &error {
            let status_code = *status_code;
            return self.transition_classified_failure(
                claimed,
                request,
                classify_http_status(status_code),
            );
        }
        self.transition_classified_failure(claimed, request, classify_execution_error(&error))
    }

    #[allow(clippy::too_many_arguments)]
    fn block_claimed_execution(
        &self,
        claimed: PostTurnGovernanceJobV3,
        request: &MemoryLearningCycleRequest,
        kind: MemoryGovernanceBlockKind,
        reason: crate::PostTurnGovernanceExecutionBlockReasonV1,
        credential_ref_safe_id: Option<String>,
        credential_generation: Option<u64>,
        provider_permission_generation: Option<u64>,
    ) -> Result<MemoryLearningCycleOutcome> {
        let execution_block_authority =
            crate::PostTurnGovernanceExecutionBlockAuthorityV1::exact_binding_block(
                &claimed.execution_binding,
                credential_ref_safe_id,
                reason,
                credential_generation,
                provider_permission_generation,
                self.runtime.governance_now_secs(),
            )?;
        let blocked = self
            .runtime
            .block_claimed_governance_job(MemoryGovernanceClaimedJobBlockRequest {
                job_id: claimed.job_id,
                lease_owner: request.lease_owner.clone(),
                lease_epoch: claimed.lease_epoch,
                kind,
                reason: reason.as_str().to_string(),
                execution_block_authority: Some(execution_block_authority),
            })?
            .job;
        Ok(MemoryLearningCycleOutcome::Blocked(
            MemoryLearningStateReport {
                job: blocked,
                reason: reason.as_str().to_string(),
            },
        ))
    }

    fn transition_classified_failure(
        &self,
        claimed: PostTurnGovernanceJobV3,
        request: &MemoryLearningCycleRequest,
        error_class: PostTurnGovernanceErrorClassV2,
    ) -> Result<MemoryLearningCycleOutcome> {
        if error_class.is_retryable() {
            let job = self
                .runtime
                .retry_governance_job(MemoryGovernanceJobRetryRequest {
                    job_id: claimed.job_id,
                    lease_owner: request.lease_owner.clone(),
                    lease_epoch: claimed.lease_epoch,
                    error_class,
                })?
                .job;
            if job.status == PostTurnGovernanceJobStatusV2::DeadLetter {
                return Ok(MemoryLearningCycleOutcome::Failed(
                    MemoryLearningStateReport {
                        job,
                        reason: "governance_retry_attempts_exhausted".to_string(),
                    },
                ));
            }
            return Ok(MemoryLearningCycleOutcome::Retrying(
                MemoryLearningStateReport {
                    job,
                    reason: error_class.as_str().to_string(),
                },
            ));
        }
        let job = self
            .runtime
            .fail_governance_job(MemoryGovernanceJobFailRequest {
                job_id: claimed.job_id,
                lease_owner: request.lease_owner.clone(),
                lease_epoch: claimed.lease_epoch,
                error_class,
                reason: error_class.as_str().to_string(),
            })?
            .job;
        Ok(MemoryLearningCycleOutcome::Failed(
            MemoryLearningStateReport {
                job,
                reason: error_class.as_str().to_string(),
            },
        ))
    }
}

struct RuntimeGovernanceExecutionOperation {
    runtime: Arc<MemoryRuntime>,
    request: Option<MemoryGovernanceJobRunRequest>,
    report: Option<MemoryGovernanceJobRunReport>,
}

impl GovernanceExecutionOperation for RuntimeGovernanceExecutionOperation {
    fn run(
        &mut self,
        http: &mut dyn LlmHttpClient,
        llm: &(dyn LlmClient + Send + Sync),
    ) -> Result<()> {
        let request = self.request.take().ok_or_else(|| {
            Error::conflict(
                "memory_learning_execution",
                "one governance operation cannot execute twice",
            )
        })?;
        let report = self
            .runtime
            .run_claimed_governance(http, Some(llm), request)?;
        self.report = Some(report);
        Ok(())
    }
}

fn is_due(job: &PostTurnGovernanceJobV3, now_secs: u64) -> bool {
    match job.status {
        PostTurnGovernanceJobStatusV2::Pending => job
            .next_attempt_at
            .is_none_or(|eligible| eligible <= now_secs),
        PostTurnGovernanceJobStatusV2::RetryWaiting => job
            .next_attempt_at
            .is_none_or(|eligible| eligible <= now_secs),
        PostTurnGovernanceJobStatusV2::Leased => {
            job.lease_until.is_none_or(|deadline| deadline <= now_secs)
        }
        PostTurnGovernanceJobStatusV2::BlockedConfiguration
        | PostTurnGovernanceJobStatusV2::BlockedCapability
        | PostTurnGovernanceJobStatusV2::BlockedPolicy => true,
        PostTurnGovernanceJobStatusV2::Succeeded
        | PostTurnGovernanceJobStatusV2::Cancelled
        | PostTurnGovernanceJobStatusV2::DeadLetter => false,
    }
}

fn classify_http_status(status_code: u16) -> PostTurnGovernanceErrorClassV2 {
    match status_code {
        429 => PostTurnGovernanceErrorClassV2::RateLimited,
        408 | 504 => PostTurnGovernanceErrorClassV2::Timeout,
        _ => PostTurnGovernanceErrorClassV2::ServiceUnavailable,
    }
}

fn classify_execution_error(error: &Error) -> PostTurnGovernanceErrorClassV2 {
    match error {
        Error::Http { status_code, .. } => classify_http_status(*status_code),
        Error::Io { .. }
        | Error::Other { .. }
        | Error::Nvs { .. }
        | Error::Storage { .. }
        | Error::Esp { .. } => PostTurnGovernanceErrorClassV2::ServiceUnavailable,
        Error::Config { stage, .. }
            if matches!(
                *stage,
                "governance_model_llm"
                    | "private_garden_governance_output"
                    | "long_term_memory_extraction_output"
            ) =>
        {
            PostTurnGovernanceErrorClassV2::MalformedModelOutput
        }
        Error::Conflict { .. } | Error::NotFound { .. } => {
            PostTurnGovernanceErrorClassV2::IdentityMismatch
        }
        Error::InvalidInput { .. } | Error::Config { .. } => {
            PostTurnGovernanceErrorClassV2::SchemaViolation
        }
    }
}

fn classify_procedural_error(error: &Error) -> (ProceduralFeedbackErrorClassV1, bool) {
    // Public redaction and durable scheduling consume one classification owner;
    // wrapping or unwrapping a Store error cannot change its recovery semantics.
    let typed = crate::ProceduralLearningSdkError::from_owner_error(
        crate::ProceduralLearningSdkOperation::ApplyFeedback,
        error,
    );
    use crate::ProceduralLearningSdkErrorDisposition as D;
    match typed.disposition {
        D::RepairRequired | D::ContractRejected => {
            (ProceduralFeedbackErrorClassV1::ClosureRejected, true)
        }
        D::AuthorityRejected => (ProceduralFeedbackErrorClassV1::AuthorityDenied, false),
        D::ExpectedStateConflict => (ProceduralFeedbackErrorClassV1::CasConflict, false),
        D::CapacityRejected => (ProceduralFeedbackErrorClassV1::BudgetExceeded, false),
        D::RegistryRejected => (ProceduralFeedbackErrorClassV1::RegistryUnavailable, false),
        D::StoreCommitRejected => (ProceduralFeedbackErrorClassV1::StoreUnavailable, false),
    }
}

#[cfg(test)]
mod procedural_error_tests {
    use super::*;

    #[test]
    fn pfi2_budget_readmission_preserves_retry_but_never_washes_identity_damage() {
        use bm_core::budget::{RuntimeBudgetAuthority, RuntimeStoreMedium, StaticPlatformManifest};
        let profile = match std::env::consts::OS {
            "macos" => crate::ProfileId::DesktopMacosEmbeddedSdk,
            "linux" => crate::ProfileId::DesktopLinuxEmbeddedSdk,
            "windows" => crate::ProfileId::DesktopWindowsEmbeddedSdk,
            other => panic!("no production host profile for {other}"),
        };
        let authority = RuntimeBudgetAuthority::with_default_host_probe(
            profile,
            StaticPlatformManifest::for_profile(profile, RuntimeStoreMedium::VolatileMemory),
            None,
            1,
        )
        .expect("real runtime budget authority");
        let report = authority.current_report(1);
        report
            .validate_for_admission(1)
            .expect("positive fresh admission");
        let expired = report
            .validate_for_admission(u64::MAX)
            .expect_err("natural expiry");
        let expected = (ProceduralFeedbackErrorClassV1::StoreUnavailable, false);
        assert_eq!(classify_procedural_error(&expired), expected);
        let wrapped = Error::storage("synthetic_admission_wrapper", expired);
        assert_eq!(classify_procedural_error(&wrapped), expected);
        let safe = crate::ProceduralLearningSdkError::from_owner_error(
            crate::ProceduralLearningSdkOperation::ProducerControl,
            &wrapped,
        );
        assert_eq!(classify_procedural_error(&safe.into_core_error()), expected);

        let mut tampered = report;
        tampered.store_budget.kv_max_entries =
            tampered.store_budget.kv_max_entries.saturating_add(1);
        let error = tampered
            .validate_for_admission(u64::MAX)
            .expect_err("tamper despite expiry");
        assert_eq!(
            classify_procedural_error(&error),
            (ProceduralFeedbackErrorClassV1::ClosureRejected, true)
        );
        for stage in [
            "runtime_budget_admission",
            "memory_write_transaction_resource_admission",
        ] {
            let error = Error::config(
                stage,
                "same stage is not proof of a valid expired admission",
            );
            assert_eq!(
                classify_procedural_error(&error),
                (ProceduralFeedbackErrorClassV1::ClosureRejected, true)
            );
        }
    }

    #[test]
    fn pfi2_consistent_read_capacity_is_not_transient_storage_failure() {
        let capacity = Error::config(
            "store_consistent_read_budget_exceeded",
            "synthetic private read detail",
        );
        assert_eq!(
            classify_procedural_error(&capacity),
            (ProceduralFeedbackErrorClassV1::BudgetExceeded, false)
        );
        let safe = crate::ProceduralLearningSdkError::from_owner_error(
            crate::ProceduralLearningSdkOperation::ApplyFeedback,
            &capacity,
        );
        assert_eq!(
            safe.disposition,
            crate::ProceduralLearningSdkErrorDisposition::CapacityRejected
        );
        assert!(!safe.to_string().contains("synthetic private read detail"));
    }

    #[test]
    fn pfi2_typed_repair_and_conflict_are_not_transient_store_failures() {
        for (key, disposition, expected) in [
            (
                crate::ProceduralLearningErrorKeyV1::RepairRequired,
                crate::ProceduralLearningSdkErrorDisposition::RepairRequired,
                (ProceduralFeedbackErrorClassV1::ClosureRejected, true),
            ),
            (
                crate::ProceduralLearningErrorKeyV1::EvidenceConflict,
                crate::ProceduralLearningSdkErrorDisposition::ExpectedStateConflict,
                (ProceduralFeedbackErrorClassV1::CasConflict, false),
            ),
        ] {
            let error = Error::Other {
                stage: "post_turn_learning_evidence",
                source: Box::new(crate::ProceduralLearningSdkError {
                    operation: crate::ProceduralLearningSdkOperation::FinalizeTurn,
                    key,
                    disposition,
                }),
            };
            assert_eq!(classify_procedural_error(&error), expected);
        }
    }

    #[test]
    fn pfi2_source_error_redaction_preserves_retry_capacity_and_authority_classes() {
        const PRIVATE_MARKER: &str = "synthetic-private-error-chain-271828";
        let examples = [
            (
                Error::config("recall_immutable_read_session", PRIVATE_MARKER),
                ProceduralFeedbackErrorClassV1::ClosureRejected,
                true,
            ),
            (
                Error::config("store_budget_exceeded", PRIVATE_MARKER),
                ProceduralFeedbackErrorClassV1::BudgetExceeded,
                false,
            ),
            (
                Error::conflict("memory_mutation_cas", PRIVATE_MARKER),
                ProceduralFeedbackErrorClassV1::CasConflict,
                false,
            ),
            (
                Error::conflict("procedural_producer_authority", PRIVATE_MARKER),
                ProceduralFeedbackErrorClassV1::AuthorityDenied,
                false,
            ),
            (
                Error::Io {
                    stage: "synthetic_source_read",
                    source: std::io::Error::other(PRIVATE_MARKER),
                },
                ProceduralFeedbackErrorClassV1::StoreUnavailable,
                false,
            ),
            (
                Error::Io {
                    stage: "synthetic_source_read",
                    source: std::io::Error::new(std::io::ErrorKind::InvalidData, PRIVATE_MARKER),
                },
                ProceduralFeedbackErrorClassV1::ClosureRejected,
                true,
            ),
        ];
        for (original, class, repair) in examples {
            for operation in [
                crate::ProceduralLearningSdkOperation::ProducerControl,
                crate::ProceduralLearningSdkOperation::FinalizeTurn,
                crate::ProceduralLearningSdkOperation::ApplyFeedback,
            ] {
                let safe =
                    crate::ProceduralLearningSdkError::from_owner_error(operation, &original);
                assert_eq!(safe.operation, operation);
                assert!(!format!("{safe}\n{safe:?}").contains(PRIVATE_MARKER));
                assert!(std::error::Error::source(&safe).is_none());
                assert_eq!(
                    classify_procedural_error(&safe.into_core_error()),
                    (class, repair)
                );
            }
        }
        let nested = Error::storage(
            "synthetic_source_read",
            Error::config("store_budget_exceeded", PRIVATE_MARKER),
        );
        let safe = crate::ProceduralLearningSdkError::from_owner_error(
            crate::ProceduralLearningSdkOperation::ApplyFeedback,
            &nested,
        );
        assert_eq!(
            classify_procedural_error(&safe.into_core_error()),
            (ProceduralFeedbackErrorClassV1::BudgetExceeded, false)
        );
    }
}
