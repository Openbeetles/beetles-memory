use serde::{Deserialize, Serialize};

use crate::orchestrator::PressureLevel;
use crate::runtime::RuntimeLifecycleModeInput;

use super::{CanonicalTurnDelta, PrivateGardenGovernanceManifestEntry, SessionTurnCommitReport};

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
pub struct DeferredGovernanceJob {
    pub job_id: String,
    pub idempotency_key: String,
    pub status: DeferredGovernanceJobStatus,
    #[serde(default)]
    pub memory_space_id: String,
    #[serde(default)]
    pub subject_id: String,
    #[serde(default)]
    pub channel: String,
    pub chat_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub turn_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidate_ids: Vec<String>,
    pub reason: String,
    #[serde(default)]
    pub retry_policy: String,
    pub created_at: u64,
    pub attempts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<CanonicalTurnDelta>,
    #[serde(default)]
    pub tool_calls: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_skill_selected_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub task_learning_selected_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reuse_outcome_note: String,
    #[serde(default)]
    pub pressure: PressureLevel,
    #[serde(default)]
    pub mode_input: RuntimeLifecycleModeInput,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
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
    pub fn from_job(job: &DeferredGovernanceJob) -> Self {
        Self {
            job_id: job.job_id.clone(),
            idempotency_key: job.idempotency_key.clone(),
            status: job.status,
            memory_space_id: job.memory_space_id.clone(),
            subject_id: job.subject_id.clone(),
            channel: job.channel.clone(),
            chat_id: job.chat_id.clone(),
            conversation_id: job.conversation_id.clone(),
            turn_id: job.turn_id.clone(),
            candidate_ids: job.candidate_ids.clone(),
            reason: job.reason.clone(),
            retry_policy: job.retry_policy.clone(),
            created_at: job.created_at,
            attempts: job.attempts,
            last_error: job.last_error.clone(),
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
    jobs: &[DeferredGovernanceJob],
    recent_limit: usize,
) -> DeferredGovernanceQueueReport {
    let mut report = DeferredGovernanceQueueReport {
        total: jobs.len(),
        ..DeferredGovernanceQueueReport::default()
    };
    for job in jobs {
        match job.status {
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
            job.status,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostTurnMemoryGovernanceReport<MaintenanceReport, LifecycleReport> {
    pub session_commit: SessionTurnCommitReport,
    pub maintenance: Option<MaintenanceReport>,
    pub private_garden_self_work: PostTurnPrivateGardenReport,
    pub semantic_governance: PostTurnSemanticGovernanceReport,
    pub lifecycle_report: LifecycleReport,
}
