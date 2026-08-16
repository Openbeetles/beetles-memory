use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{hash_map::DefaultHasher, BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

use crate::error::{Error, Result};

use super::governed_post_image::{
    revision_is_exact_successor, GovernedDocumentImage, GovernedPostImageValidation,
};
use super::long_term::scoped_long_term_control_storage_key;
use super::long_term_version::{
    LongTermMemoryHeadManifest, LongTermMemoryVersionMaterialImage, LongTermVersionRetentionLease,
};
use super::{
    long_term_memory_evidence_summary, plan_long_term_memory_owner_mutation,
    plan_long_term_memory_upsert, DerivedMemoryPlane, DerivedMemoryRef, FacetReportView,
    GovernedContractFailure, GovernedContractValidation, GovernedMemoryOwnerPlane,
    GovernedMemoryOwnerRef, GovernedOwnerRevisionRef, GovernedOwnerTermination,
    GovernedOwnerTransition, LongTermMemoryDraft, LongTermMemoryEntry, LongTermMemoryEntryPlan,
    LongTermMemoryKind, LongTermMemoryOwnerMutation, LongTermMemoryQuery, LongTermMemoryReadStore,
    LongTermMemorySlot, LongTermMemorySourceScope, LongTermMemoryStaleHint, LongTermMemoryStore,
    LongTermMemoryVersionMaterial, MemoryPrivacyClass, MemorySpaceId,
    MemorySubjectVisibilityPolicy, SubjectId, TranscriptEvidenceRef,
};

fn plan_or_apply_owner_mutation(
    store: &dyn LongTermMemoryStore,
    previous: &LongTermMemoryEntry,
    mutation: &LongTermMemoryOwnerMutation,
    now_secs: u64,
    dry_run: bool,
) -> Result<LongTermMemoryEntryPlan> {
    if dry_run {
        Ok(plan_long_term_memory_owner_mutation(
            previous, mutation, now_secs,
        ))
    } else {
        store.mutate_owner(&previous.id, mutation, now_secs)
    }
}

pub const LONG_TERM_CONTROL_REVISION_NAMESPACE: &str = "long_term_control_revision";
pub const LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE: &str = "long_term_control_tombstone";
pub const LONG_TERM_GOVERNANCE_POLICY_NAMESPACE: &str = "long_term_governance_policy";
pub const LONG_TERM_CONTROL_AUDIT_NAMESPACE: &str = "long_term_control_audit";
pub const LONG_TERM_CONTROL_SCHEMA_VERSION: u32 = 3;
pub const LONG_TERM_CONTROL_TOMBSTONE_SCHEMA_VERSION: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LongTermControlOperation {
    Refresh,
    Correct,
    Supersede,
    Invalidate,
    Delete,
    ForgetByQuery,
    MarkStale,
    ChangeScope,
    ChangePrivacy,
    PolicySuppress,
    PolicyPause,
    PolicyResume,
    PolicyRemoveSuppression,
}

impl LongTermControlOperation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Refresh => "refresh",
            Self::Correct => "correct",
            Self::Supersede => "supersede",
            Self::Invalidate => "invalidate",
            Self::Delete => "delete",
            Self::ForgetByQuery => "forget_by_query",
            Self::MarkStale => "mark_stale",
            Self::ChangeScope => "change_scope",
            Self::ChangePrivacy => "change_privacy",
            Self::PolicySuppress => "policy.suppress",
            Self::PolicyPause => "policy.pause",
            Self::PolicyResume => "policy.resume",
            Self::PolicyRemoveSuppression => "policy.remove_suppression",
        }
    }

    pub fn from_label(value: &str) -> Option<Self> {
        match value.trim() {
            "refresh" => Some(Self::Refresh),
            "correct" => Some(Self::Correct),
            "supersede" => Some(Self::Supersede),
            "invalidate" => Some(Self::Invalidate),
            "delete" => Some(Self::Delete),
            "forget_by_query" => Some(Self::ForgetByQuery),
            "mark_stale" => Some(Self::MarkStale),
            "change_scope" => Some(Self::ChangeScope),
            "change_privacy" => Some(Self::ChangePrivacy),
            "policy.suppress" => Some(Self::PolicySuppress),
            "policy.pause" => Some(Self::PolicyPause),
            "policy.resume" => Some(Self::PolicyResume),
            "policy.remove_suppression" => Some(Self::PolicyRemoveSuppression),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LongTermInvalidationReasonCode {
    FactuallyIncorrect,
    SourceAuthorityRevoked,
    ContradictedByGovernedEvidence,
    IntegrityViolation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LongTermInvalidationContract {
    pub target: MemoryLongTermTarget,
    pub reason_code: LongTermInvalidationReasonCode,
    pub governed_evidence_refs: Vec<GovernedOwnerRevisionRef>,
    pub actor_subject_id: SubjectId,
    pub audit_reason: String,
}

impl LongTermInvalidationContract {
    pub fn validate_contract(&self) -> GovernedContractValidation {
        let mut failures = Vec::new();
        if self.actor_subject_id.trim().is_empty()
            || self.actor_subject_id != self.actor_subject_id.trim()
        {
            failures.push(GovernedContractFailure::InvalidationActorMissing);
        }
        if self.audit_reason.trim().is_empty() || self.audit_reason != self.audit_reason.trim() {
            failures.push(GovernedContractFailure::InvalidationAuditReasonMissing);
        }
        if self.governed_evidence_refs.is_empty() {
            failures.push(GovernedContractFailure::InvalidationEvidenceMissing);
        }
        if self.governed_evidence_refs.iter().any(|evidence| {
            !evidence.is_valid()
                || evidence.owner_ref.owner_plane != GovernedMemoryOwnerPlane::EvidenceDocument
        }) {
            failures.push(GovernedContractFailure::InvalidationEvidenceOwnerInvalid);
        }
        failures.sort();
        failures.dedup();
        GovernedContractValidation {
            accepted: failures.is_empty(),
            failures,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemoryLongTermControlView {
    HostUi,
    Operator,
    RawOwner,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryLongTermSelector {
    pub query: LongTermMemoryQuery,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_ref: Option<TranscriptEvidenceRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryLongTermTarget {
    RecordId(String),
    Slot(LongTermMemorySlot),
    TranscriptDerivedRef(DerivedMemoryRef),
    Query(MemoryLongTermSelector),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryLongTermMutation {
    Correct {
        target: MemoryLongTermTarget,
        replacement: LongTermMemoryDraft,
    },
    Supersede {
        target: MemoryLongTermTarget,
        replacement: LongTermMemoryDraft,
    },
    Invalidate {
        contract: LongTermInvalidationContract,
    },
    Delete {
        target: MemoryLongTermTarget,
    },
    ForgetByQuery {
        selector: MemoryLongTermSelector,
        confirmation_token: Option<String>,
    },
    MarkStale {
        target: MemoryLongTermTarget,
        stale_hint: LongTermMemoryStaleHint,
    },
    ChangeScope {
        target: MemoryLongTermTarget,
        source_scope: LongTermMemorySourceScope,
        subject_visibility: MemorySubjectVisibilityPolicy,
    },
    ChangePrivacy {
        target: MemoryLongTermTarget,
        privacy: MemoryPrivacyClass,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct MemoryGovernanceSelector {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_space_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_id: Option<SubjectId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<LongTermMemoryKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic_pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_chat_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<LongTermMemorySourceScope>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemoryGovernanceSuppressionDuration {
    UntilManualResume,
    UntilUnixSecs(u64),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryGovernancePolicyMutation {
    Pause {
        selector: MemoryGovernanceSelector,
        expires_at: Option<u64>,
    },
    Resume {
        selector: MemoryGovernanceSelector,
    },
    Suppress {
        selector: MemoryGovernanceSelector,
        duration: MemoryGovernanceSuppressionDuration,
    },
    RemoveSuppression {
        selector: MemoryGovernanceSelector,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LongTermMemoryControlListRequest {
    pub query: LongTermMemoryQuery,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub limit: usize,
    pub view: MemoryLongTermControlView,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LongTermMemoryControlDetailRequest {
    pub target: MemoryLongTermTarget,
    pub view: MemoryLongTermControlView,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LongTermMemoryControlMutationRequest {
    pub operation: MemoryLongTermMutation,
    pub reason: String,
    pub dry_run: bool,
    pub factual_owner_id: MemorySpaceId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_subject_id: Option<SubjectId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_space_id: Option<String>,
    pub now_secs: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryLongTermRecordReport {
    pub record: LongTermMemoryEntry,
    pub evidence_summary: String,
    pub transcript_refs: Vec<TranscriptEvidenceRef>,
    pub tombstoned: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryLongTermListReport {
    pub records: Vec<MemoryLongTermRecordReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub total_visible: usize,
    pub view: MemoryLongTermControlView,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryLongTermDetailReport {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<LongTermMemoryEntry>,
    pub revisions: Vec<LongTermMemoryControlRevision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tombstone: Option<LongTermMemoryTombstone>,
    pub transcript_refs: Vec<TranscriptEvidenceRef>,
    pub view: MemoryLongTermControlView,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryLongTermTargetResolutionReport {
    pub resolved_count: usize,
    pub ambiguous_count: usize,
    pub not_found_count: usize,
    pub resolved_record_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryLongTermAffectedRecord {
    pub record_id: String,
    pub operation: String,
    pub previous_owner_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_owner_revision: Option<u64>,
    pub previous_source_revision: Option<u64>,
    pub new_source_revision: Option<u64>,
    pub previous_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_digest: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryLongTermTombstoneRef {
    pub record_id: String,
    pub tombstone_id: String,
    pub operation: LongTermControlOperation,
    pub last_owner_revision: u64,
    pub last_source_revision: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryLongTermControlDecision {
    pub accepted: bool,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmation_token: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryProjectionImpactReport {
    pub affected_record_ids: Vec<String>,
    pub subject_visibility: MemorySubjectVisibilityPolicy,
    pub recall_projection_must_refresh: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

impl Default for MemoryProjectionImpactReport {
    fn default() -> Self {
        Self {
            affected_record_ids: Vec::new(),
            subject_visibility: MemorySubjectVisibilityPolicy::AllSubjects,
            recall_projection_must_refresh: false,
            notes: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryDeferredGovernanceImpactReport {
    pub policy_ids: Vec<String>,
    pub deferred_jobs_may_be_affected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryLongTermAffectedFacetDoc {
    pub action: String,
    pub owner_token: String,
    pub report_view: FacetReportView,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryLongTermMutationReport {
    pub accepted: bool,
    pub dry_run: bool,
    pub operation: &'static str,
    pub target_report: MemoryLongTermTargetResolutionReport,
    pub affected_records: Vec<MemoryLongTermAffectedRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub affected_facet_docs: Vec<MemoryLongTermAffectedFacetDoc>,
    pub tombstones: Vec<MemoryLongTermTombstoneRef>,
    pub evidence_refs: Vec<DerivedMemoryRef>,
    pub transcript_refs: Vec<TranscriptEvidenceRef>,
    pub policy_decision: MemoryLongTermControlDecision,
    pub projection_impact: MemoryProjectionImpactReport,
    pub deferred_governance_impact: MemoryDeferredGovernanceImpactReport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audit_event_id: Option<String>,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LongTermMemoryControlRevisionIntent {
    pub revision_id: String,
    pub memory_space_id: String,
    pub factual_owner_id: MemorySpaceId,
    pub operation: LongTermControlOperation,
    pub invalidation_reason_code: Option<LongTermInvalidationReasonCode>,
    pub transition: GovernedOwnerTransition,
    pub governed_evidence_refs: Vec<GovernedOwnerRevisionRef>,
    pub actor_subject_id: Option<SubjectId>,
    pub reason: String,
    pub created_at: u64,
}

impl LongTermMemoryControlRevisionIntent {
    #[allow(clippy::too_many_arguments)]
    pub fn for_owner_change(
        revision_id: impl Into<String>,
        operation: LongTermControlOperation,
        before: &LongTermMemoryEntry,
        after: Option<&LongTermMemoryEntry>,
        reason: impl Into<String>,
        factual_owner_id: MemorySpaceId,
        actor_subject_id: Option<SubjectId>,
        memory_space_id: impl Into<String>,
        created_at: u64,
        governed_evidence_refs: Vec<GovernedOwnerRevisionRef>,
    ) -> Result<Self> {
        Self::for_owner_change_with_invalidation_reason(
            revision_id,
            operation,
            before,
            after,
            reason,
            factual_owner_id,
            actor_subject_id,
            memory_space_id,
            created_at,
            governed_evidence_refs,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn for_invalidation(
        revision_id: impl Into<String>,
        before: &LongTermMemoryEntry,
        reason_code: LongTermInvalidationReasonCode,
        reason: impl Into<String>,
        factual_owner_id: MemorySpaceId,
        actor_subject_id: SubjectId,
        memory_space_id: impl Into<String>,
        created_at: u64,
        governed_evidence_refs: Vec<GovernedOwnerRevisionRef>,
    ) -> Result<Self> {
        Self::for_owner_change_with_invalidation_reason(
            revision_id,
            LongTermControlOperation::Invalidate,
            before,
            None,
            reason,
            factual_owner_id,
            Some(actor_subject_id),
            memory_space_id,
            created_at,
            governed_evidence_refs,
            Some(reason_code),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn for_owner_change_with_invalidation_reason(
        revision_id: impl Into<String>,
        operation: LongTermControlOperation,
        before: &LongTermMemoryEntry,
        after: Option<&LongTermMemoryEntry>,
        reason: impl Into<String>,
        factual_owner_id: MemorySpaceId,
        actor_subject_id: Option<SubjectId>,
        memory_space_id: impl Into<String>,
        created_at: u64,
        governed_evidence_refs: Vec<GovernedOwnerRevisionRef>,
        invalidation_reason_code: Option<LongTermInvalidationReasonCode>,
    ) -> Result<Self> {
        let termination = match operation {
            LongTermControlOperation::Refresh => GovernedOwnerTermination::Revised,
            LongTermControlOperation::Correct => GovernedOwnerTermination::Corrected,
            LongTermControlOperation::Supersede => GovernedOwnerTermination::Superseded,
            LongTermControlOperation::Invalidate => GovernedOwnerTermination::Invalidated,
            LongTermControlOperation::Delete => GovernedOwnerTermination::Deleted,
            LongTermControlOperation::ForgetByQuery => GovernedOwnerTermination::Forgotten,
            LongTermControlOperation::MarkStale
            | LongTermControlOperation::ChangeScope
            | LongTermControlOperation::ChangePrivacy => GovernedOwnerTermination::Revised,
            LongTermControlOperation::PolicySuppress
            | LongTermControlOperation::PolicyPause
            | LongTermControlOperation::PolicyResume
            | LongTermControlOperation::PolicyRemoveSuppression => {
                return Err(Error::config(
                    "long_term_control_revision_intent",
                    "policy operations do not create owner version transitions",
                ));
            }
        };
        let predecessor = GovernedOwnerRevisionRef::try_new(
            GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, before.id.clone()),
            before.owner_revision,
        )?;
        let successor = after
            .map(|entry| {
                GovernedOwnerRevisionRef::try_new(
                    GovernedMemoryOwnerRef::new(
                        GovernedMemoryOwnerPlane::LongTerm,
                        entry.id.clone(),
                    ),
                    entry.owner_revision,
                )
            })
            .transpose()?;
        let mut governed_evidence_refs = governed_evidence_refs;
        governed_evidence_refs.sort();
        governed_evidence_refs.dedup();
        let intent = Self {
            revision_id: revision_id.into(),
            memory_space_id: memory_space_id.into(),
            factual_owner_id,
            operation,
            invalidation_reason_code,
            transition: GovernedOwnerTransition {
                predecessor,
                terminated_at: created_at,
                termination,
                successor,
            },
            governed_evidence_refs,
            actor_subject_id,
            reason: reason.into(),
            created_at,
        };
        intent.validate_contract()?;
        Ok(intent)
    }

    pub fn validate_contract(&self) -> Result<()> {
        if self.revision_id.trim().is_empty()
            || self.revision_id != self.revision_id.trim()
            || self.memory_space_id.trim().is_empty()
            || self.memory_space_id != self.memory_space_id.trim()
            || self.factual_owner_id.trim().is_empty()
            || self.factual_owner_id != self.factual_owner_id.trim()
            || self.factual_owner_id != self.memory_space_id
            || self.reason.trim().is_empty()
            || self.reason != self.reason.trim()
            || self.created_at == 0
            || self.transition.terminated_at < self.created_at
            || (self.operation == LongTermControlOperation::Invalidate)
                != self.invalidation_reason_code.is_some()
            || self
                .governed_evidence_refs
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || self.governed_evidence_refs.iter().any(|evidence| {
                !evidence.is_valid()
                    || evidence.owner_ref.owner_plane != GovernedMemoryOwnerPlane::EvidenceDocument
            })
        {
            return Err(Error::config(
                "long_term_control_revision_intent",
                "revision intent identity, scope, transition, evidence, reason, or time is invalid",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LongTermMemoryControlRevision {
    pub schema_version: u32,
    pub revision_id: String,
    pub memory_space_id: String,
    pub factual_owner_id: MemorySpaceId,
    pub operation: LongTermControlOperation,
    pub invalidation_reason_code: Option<LongTermInvalidationReasonCode>,
    pub transition: GovernedOwnerTransition,
    pub predecessor_material_digest: String,
    pub successor_material_digest: Option<String>,
    pub governed_evidence_refs: Vec<GovernedOwnerRevisionRef>,
    pub reason: String,
    pub actor_subject_id: Option<SubjectId>,
    pub created_at: u64,
    pub content_digest: String,
}

impl LongTermMemoryControlRevision {
    pub fn bind(
        mut intent: LongTermMemoryControlRevisionIntent,
        predecessor: &LongTermMemoryVersionMaterial,
        successor: Option<&LongTermMemoryVersionMaterial>,
    ) -> Result<Self> {
        intent.validate_contract()?;
        let monotonic_successor_time =
            predecessor
                .origin
                .valid_from
                .checked_add(1)
                .ok_or_else(|| {
                    Error::config(
                        "long_term_control_revision",
                        "predecessor validity time cannot advance without overflow",
                    )
                })?;
        intent.transition.terminated_at = intent.created_at.max(monotonic_successor_time);
        let transition_validation = intent.transition.validate_contract(predecessor, successor);
        if !transition_validation.accepted
            || predecessor.memory_space_id != intent.memory_space_id
            || predecessor.factual_owner_id != intent.factual_owner_id
        {
            return Err(Error::config(
                "long_term_control_revision",
                format!(
                    "material transition binding rejected: {:?}",
                    transition_validation.failures
                ),
            ));
        }
        let mut revision = Self {
            schema_version: LONG_TERM_CONTROL_SCHEMA_VERSION,
            revision_id: intent.revision_id,
            memory_space_id: intent.memory_space_id,
            factual_owner_id: intent.factual_owner_id,
            operation: intent.operation,
            invalidation_reason_code: intent.invalidation_reason_code,
            transition: intent.transition,
            predecessor_material_digest: predecessor.content_digest.clone(),
            successor_material_digest: successor.map(|material| material.content_digest.clone()),
            governed_evidence_refs: intent.governed_evidence_refs,
            reason: intent.reason,
            actor_subject_id: intent.actor_subject_id,
            created_at: intent.created_at,
            content_digest: String::new(),
        };
        revision.content_digest = revision.canonical_content_digest()?;
        Ok(revision)
    }

    pub fn canonical_content_digest(&self) -> Result<String> {
        #[derive(Serialize)]
        struct DigestInput<'a> {
            schema_version: u32,
            revision_id: &'a str,
            memory_space_id: &'a str,
            factual_owner_id: &'a str,
            operation: LongTermControlOperation,
            invalidation_reason_code: Option<LongTermInvalidationReasonCode>,
            transition: &'a GovernedOwnerTransition,
            predecessor_material_digest: &'a str,
            successor_material_digest: Option<&'a str>,
            governed_evidence_refs: &'a [GovernedOwnerRevisionRef],
            reason: &'a str,
            actor_subject_id: Option<&'a str>,
            created_at: u64,
        }
        let encoded = serde_json::to_vec(&DigestInput {
            schema_version: self.schema_version,
            revision_id: &self.revision_id,
            memory_space_id: &self.memory_space_id,
            factual_owner_id: &self.factual_owner_id,
            operation: self.operation,
            invalidation_reason_code: self.invalidation_reason_code,
            transition: &self.transition,
            predecessor_material_digest: &self.predecessor_material_digest,
            successor_material_digest: self.successor_material_digest.as_deref(),
            governed_evidence_refs: &self.governed_evidence_refs,
            reason: &self.reason,
            actor_subject_id: self.actor_subject_id.as_deref(),
            created_at: self.created_at,
        })
        .map_err(|error| Error::config("long_term_control_revision", error.to_string()))?;
        let mut hasher = Sha256::new();
        hash_control_field(&mut hasher, b"long_term_control_revision_v3");
        hash_control_field(&mut hasher, &encoded);
        Ok(format!("{:x}", hasher.finalize()))
    }

    pub fn validate_contract(&self) -> Result<()> {
        if self.schema_version != LONG_TERM_CONTROL_SCHEMA_VERSION
            || self.revision_id.trim().is_empty()
            || self.memory_space_id.trim().is_empty()
            || self.factual_owner_id.trim().is_empty()
            || self.factual_owner_id != self.memory_space_id
            || self.reason.trim().is_empty()
            || self.created_at == 0
            || self.transition.terminated_at < self.created_at
            || (self.operation == LongTermControlOperation::Invalidate)
                != self.invalidation_reason_code.is_some()
            || !is_control_sha256(&self.predecessor_material_digest)
            || self
                .successor_material_digest
                .as_ref()
                .is_some_and(|digest| !is_control_sha256(digest))
            || self.transition.successor.is_some() != self.successor_material_digest.is_some()
            || self
                .governed_evidence_refs
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || self.canonical_content_digest().ok().as_deref() != Some(self.content_digest.as_str())
        {
            return Err(Error::config(
                "long_term_control_revision",
                "typed control revision contract is invalid",
            ));
        }
        Ok(())
    }
}

fn hash_control_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn is_control_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LongTermMemoryTombstone {
    pub schema_version: u32,
    pub tombstone_id: String,
    pub record_id: String,
    pub operation: LongTermControlOperation,
    pub last_owner_revision: u64,
    pub last_source_revision: Option<u64>,
    pub previous_digest: String,
    pub subject_visibility: MemorySubjectVisibilityPolicy,
    pub reason: String,
    pub factual_owner_id: MemorySpaceId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_subject_id: Option<SubjectId>,
    pub memory_space_id: String,
    pub created_at: u64,
}

impl LongTermMemoryTombstone {
    pub fn validate_contract(&self) -> Result<()> {
        if self.schema_version != LONG_TERM_CONTROL_TOMBSTONE_SCHEMA_VERSION
            || self.tombstone_id.trim().is_empty()
            || self.tombstone_id != self.tombstone_id.trim()
            || self.record_id.trim().is_empty()
            || self.record_id != self.record_id.trim()
            || !matches!(
                self.operation,
                LongTermControlOperation::Supersede
                    | LongTermControlOperation::Delete
                    | LongTermControlOperation::ForgetByQuery
            )
            || self.last_owner_revision == 0
            || !is_control_sha256(&self.previous_digest)
            || self.subject_visibility.validate_canonical().is_err()
            || self.reason.trim().is_empty()
            || self.reason != self.reason.trim()
            || self.factual_owner_id.trim().is_empty()
            || self.factual_owner_id != self.factual_owner_id.trim()
            || self.factual_owner_id != self.memory_space_id
            || self.memory_space_id.trim().is_empty()
            || self.memory_space_id != self.memory_space_id.trim()
            || self.created_at == 0
        {
            return Err(Error::config(
                "long_term_control_tombstone",
                "typed tombstone identity, operation, digest, scope, reason, or time is invalid",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryLongTermGovernancePolicy {
    pub schema_version: u32,
    pub policy_revision: u64,
    pub memory_space_id: String,
    pub policy_id: String,
    pub kind: String,
    pub selector: MemoryGovernanceSelector,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<MemoryGovernanceSuppressionDuration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    pub reason: String,
    pub created_at: u64,
    pub updated_at: u64,
}

impl MemoryLongTermGovernancePolicy {
    pub fn matches_candidate(
        &self,
        memory_space_id: Option<&str>,
        subject_id: Option<&str>,
        kind: &LongTermMemoryKind,
        topic: &str,
        source_chat_id: Option<&str>,
        source_scope: LongTermMemorySourceScope,
    ) -> bool {
        selector_matches_candidate(
            &self.selector,
            memory_space_id,
            subject_id,
            kind,
            topic,
            source_chat_id,
            source_scope,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryGovernancePolicyMutationReport {
    pub accepted: bool,
    pub dry_run: bool,
    pub operation: &'static str,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_id: Option<String>,
    pub affected_future_writes: String,
    pub policy_decision: MemoryLongTermControlDecision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audit_event_id: Option<String>,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LongTermMemoryControlAuditEvent {
    pub schema_version: u32,
    pub event_id: String,
    pub transaction_id: String,
    pub operation: LongTermControlOperation,
    pub effects: Vec<ControlEffectRef>,
    pub reason: String,
    pub factual_owner_id: MemorySpaceId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_subject_id: Option<SubjectId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_space_id: Option<String>,
    pub created_at: u64,
}

impl LongTermMemoryControlAuditEvent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        event_id: impl Into<String>,
        transaction_id: impl Into<String>,
        operation: LongTermControlOperation,
        mut effects: Vec<ControlEffectRef>,
        reason: impl Into<String>,
        factual_owner_id: MemorySpaceId,
        actor_subject_id: Option<SubjectId>,
        memory_space_id: impl Into<String>,
        created_at: u64,
    ) -> Self {
        effects.sort();
        effects.dedup();
        Self {
            schema_version: LONG_TERM_CONTROL_SCHEMA_VERSION,
            event_id: event_id.into(),
            transaction_id: transaction_id.into(),
            operation,
            effects,
            reason: reason.into(),
            factual_owner_id,
            actor_subject_id,
            memory_space_id: Some(memory_space_id.into()),
            created_at,
        }
    }

    pub fn bind_canonical_event_id(&mut self) -> Result<()> {
        self.effects.sort();
        self.effects.dedup();
        let identity = serde_json::to_vec(&(
            self.schema_version,
            self.operation,
            &self.effects,
            &self.reason,
            &self.factual_owner_id,
            &self.actor_subject_id,
            &self.memory_space_id,
            self.created_at,
        ))
        .map_err(|error| {
            Error::config(
                "long_term_control_audit_identity",
                format!("cannot canonicalize audit identity: {error}"),
            )
        })?;
        self.event_id = format!("ltma-{:x}", Sha256::digest(identity));
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ControlEffectRef {
    Revision {
        revision_id: String,
        transition: GovernedOwnerTransition,
        factual_owner_id: MemorySpaceId,
    },
    Tombstone {
        tombstone_id: String,
        record_id: String,
        factual_owner_id: MemorySpaceId,
        owner_revision: u64,
        source_revision: Option<u64>,
    },
    Policy {
        policy_id: String,
        factual_owner_id: MemorySpaceId,
        policy_revision: u64,
        deleted: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LongTermVersionOwnerSnapshot {
    pub head: LongTermMemoryHeadManifest,
    pub retained_materials: Vec<LongTermMemoryVersionMaterial>,
    pub transitions: Vec<GovernedOwnerTransition>,
}

impl LongTermVersionOwnerSnapshot {
    fn current_material(
        &self,
        lease: LongTermVersionRetentionLease,
    ) -> Result<&LongTermMemoryVersionMaterial> {
        let validation = super::long_term_version::validate_long_term_version_head_closure(
            &self.head,
            &self.retained_materials,
            &self.transitions,
            lease.max_retained_revisions_per_owner(),
        );
        if !validation.accepted || self.head.terminal_transition_ref.is_some() {
            return Err(Error::config(
                "long_term_version_snapshot",
                format!(
                    "exact active owner closure rejected: {:?}",
                    validation.failures
                ),
            ));
        }
        self.retained_materials
            .iter()
            .find(|material| {
                material.owner_ref == self.head.owner_ref
                    && material.owner_revision == self.head.current_revision
            })
            .ok_or_else(|| {
                Error::config(
                    "long_term_version_snapshot",
                    "current material is missing from the exact owner closure",
                )
            })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LongTermMemoryVersionMutationIntent {
    pub control_revision_intent: LongTermMemoryControlRevisionIntent,
    pub successor_projection: Option<LongTermMemoryEntry>,
    pub audit_transaction_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoundLongTermVersionRetention {
    AppendSuccessor,
    StartSuccessorOwner,
    RetainOperatorOnly,
    PurgeOwner,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundLongTermVersionReportIdentity {
    pub operation: LongTermControlOperation,
    pub predecessor: GovernedOwnerRevisionRef,
    pub successor: Option<GovernedOwnerRevisionRef>,
    pub predecessor_material_digest: String,
    pub successor_material_digest: Option<String>,
    pub control_revision_id: String,
    pub tombstone_id: Option<String>,
    pub audit_event_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundVersionMutation {
    pub effective_at: u64,
    pub predecessor_material: LongTermMemoryVersionMaterial,
    pub successor_material: Option<LongTermMemoryVersionMaterial>,
    pub control_revision: LongTermMemoryControlRevision,
    pub tombstone: Option<LongTermMemoryTombstone>,
    pub audit: LongTermMemoryControlAuditEvent,
    pub retention: BoundLongTermVersionRetention,
    pub report_identity: BoundLongTermVersionReportIdentity,
}

pub fn bind_long_term_version_mutation(
    intent: LongTermMemoryVersionMutationIntent,
    snapshot: &LongTermVersionOwnerSnapshot,
    lease: LongTermVersionRetentionLease,
) -> Result<BoundVersionMutation> {
    intent.control_revision_intent.validate_contract()?;
    if intent.audit_transaction_id.trim().is_empty()
        || intent.audit_transaction_id != intent.audit_transaction_id.trim()
    {
        return Err(Error::config(
            "long_term_version_mutation",
            "audit transaction identity must be canonical and non-empty",
        ));
    }

    let predecessor = snapshot.current_material(lease)?.clone();
    let control_intent = intent.control_revision_intent;
    if control_intent.memory_space_id != predecessor.memory_space_id
        || control_intent.factual_owner_id != predecessor.factual_owner_id
        || control_intent.transition.predecessor != predecessor.owner_revision_ref()
    {
        return Err(Error::config(
            "long_term_version_mutation",
            "control intent differs from the exact predecessor scope or revision",
        ));
    }

    let operation = control_intent.operation;
    let successor_required = matches!(
        operation,
        LongTermControlOperation::Refresh
            | LongTermControlOperation::Correct
            | LongTermControlOperation::Supersede
            | LongTermControlOperation::MarkStale
            | LongTermControlOperation::ChangeScope
            | LongTermControlOperation::ChangePrivacy
    );
    let terminal_without_successor = matches!(
        operation,
        LongTermControlOperation::Invalidate
            | LongTermControlOperation::Delete
            | LongTermControlOperation::ForgetByQuery
    );
    if successor_required != intent.successor_projection.is_some()
        || (!successor_required && !terminal_without_successor)
    {
        return Err(Error::config(
            "long_term_version_mutation",
            "operation and successor projection do not form a supported owner transition",
        ));
    }
    if operation == LongTermControlOperation::Invalidate
        && (control_intent.governed_evidence_refs.is_empty()
            || control_intent.actor_subject_id.is_none()
            || control_intent.invalidation_reason_code.is_none())
    {
        return Err(Error::config(
            "long_term_version_invalidation",
            "invalidation requires a typed reason, governed evidence and actor",
        ));
    }

    let effective_at =
        control_intent
            .created_at
            .max(
                predecessor
                    .origin
                    .valid_from
                    .checked_add(1)
                    .ok_or_else(|| {
                        Error::config(
                            "long_term_version_mutation",
                            "predecessor validity time cannot advance without overflow",
                        )
                    })?,
            );
    let mut governed_evidence_refs = predecessor.governed_evidence_refs.clone();
    governed_evidence_refs.extend(control_intent.governed_evidence_refs.iter().cloned());
    governed_evidence_refs.sort();
    governed_evidence_refs.dedup();
    let successor_material = intent
        .successor_projection
        .as_ref()
        .map(|successor| {
            LongTermMemoryVersionMaterial::from_current_projection(
                &predecessor.memory_space_id,
                &predecessor.factual_owner_id,
                successor,
                effective_at,
                Some(predecessor.owner_revision_ref()),
                governed_evidence_refs.clone(),
            )
        })
        .transpose()?;

    let retention = match operation {
        LongTermControlOperation::Supersede => {
            if successor_material
                .as_ref()
                .is_none_or(|successor| successor.owner_revision != 1)
            {
                return Err(Error::config(
                    "long_term_version_mutation",
                    "supersede requires a revision-one successor owner",
                ));
            }
            BoundLongTermVersionRetention::StartSuccessorOwner
        }
        LongTermControlOperation::Invalidate => BoundLongTermVersionRetention::RetainOperatorOnly,
        LongTermControlOperation::Delete | LongTermControlOperation::ForgetByQuery => {
            BoundLongTermVersionRetention::PurgeOwner
        }
        _ => {
            if snapshot.retained_materials.len() >= lease.max_retained_revisions_per_owner() {
                return Err(Error::config(
                    "long_term_version_retention",
                    "request-pinned retention limit is exhausted",
                ));
            }
            BoundLongTermVersionRetention::AppendSuccessor
        }
    };

    let control_revision = LongTermMemoryControlRevision::bind(
        control_intent,
        &predecessor,
        successor_material.as_ref(),
    )?;
    if control_revision.transition.terminated_at != effective_at {
        return Err(Error::config(
            "long_term_version_mutation",
            "control revision did not bind the exact monotonic effective time",
        ));
    }

    let tombstone = if matches!(
        operation,
        LongTermControlOperation::Supersede
            | LongTermControlOperation::Delete
            | LongTermControlOperation::ForgetByQuery
    ) {
        let tombstone_id = canonical_long_term_tombstone_id(
            operation,
            &predecessor,
            &control_revision.revision_id,
            effective_at,
        )?;
        let tombstone = LongTermMemoryTombstone {
            schema_version: LONG_TERM_CONTROL_TOMBSTONE_SCHEMA_VERSION,
            tombstone_id,
            record_id: predecessor.owner_ref.owner_id.clone(),
            operation,
            last_owner_revision: predecessor.owner_revision,
            last_source_revision: predecessor.governed_content.source_revision,
            previous_digest: predecessor.content_digest.clone(),
            subject_visibility: predecessor.subject_visibility.clone(),
            reason: control_revision.reason.clone(),
            factual_owner_id: predecessor.factual_owner_id.clone(),
            actor_subject_id: control_revision.actor_subject_id.clone(),
            memory_space_id: predecessor.memory_space_id.clone(),
            created_at: effective_at,
        };
        tombstone.validate_contract()?;
        Some(tombstone)
    } else {
        None
    };

    let mut effects = vec![ControlEffectRef::Revision {
        revision_id: control_revision.revision_id.clone(),
        transition: control_revision.transition.clone(),
        factual_owner_id: control_revision.factual_owner_id.clone(),
    }];
    if let Some(tombstone) = &tombstone {
        effects.push(ControlEffectRef::Tombstone {
            tombstone_id: tombstone.tombstone_id.clone(),
            record_id: tombstone.record_id.clone(),
            factual_owner_id: tombstone.factual_owner_id.clone(),
            owner_revision: tombstone.last_owner_revision,
            source_revision: tombstone.last_source_revision,
        });
    }
    let mut audit = LongTermMemoryControlAuditEvent::new(
        "pending",
        intent.audit_transaction_id,
        operation,
        effects,
        control_revision.reason.clone(),
        predecessor.factual_owner_id.clone(),
        control_revision.actor_subject_id.clone(),
        predecessor.memory_space_id.clone(),
        control_revision.created_at,
    );
    audit.bind_canonical_event_id()?;

    let report_identity = BoundLongTermVersionReportIdentity {
        operation,
        predecessor: control_revision.transition.predecessor.clone(),
        successor: control_revision.transition.successor.clone(),
        predecessor_material_digest: control_revision.predecessor_material_digest.clone(),
        successor_material_digest: control_revision.successor_material_digest.clone(),
        control_revision_id: control_revision.revision_id.clone(),
        tombstone_id: tombstone
            .as_ref()
            .map(|tombstone| tombstone.tombstone_id.clone()),
        audit_event_id: audit.event_id.clone(),
    };
    Ok(BoundVersionMutation {
        effective_at,
        predecessor_material: predecessor,
        successor_material,
        control_revision,
        tombstone,
        audit,
        retention,
        report_identity,
    })
}

pub fn bind_long_term_control_audit_batch(
    transaction_id: &str,
    audits: &[LongTermMemoryControlAuditEvent],
) -> Result<Vec<LongTermMemoryControlAuditEvent>> {
    if transaction_id.trim().is_empty() || transaction_id != transaction_id.trim() {
        return Err(Error::config(
            "long_term_control_audit_batch",
            "canonical transaction identity is required",
        ));
    }
    type AuditGroupKey = (
        LongTermControlOperation,
        String,
        SubjectId,
        Option<SubjectId>,
        String,
        u64,
    );
    let mut groups = BTreeMap::<AuditGroupKey, Vec<ControlEffectRef>>::new();
    for audit in audits {
        let memory_space_id = audit.memory_space_id.clone().ok_or_else(|| {
            Error::config(
                "long_term_control_audit_batch",
                "bound audit is missing its memory space",
            )
        })?;
        let mut canonical = audit.clone();
        canonical.bind_canonical_event_id()?;
        if canonical.event_id != audit.event_id || audit.effects.is_empty() {
            return Err(Error::config(
                "long_term_control_audit_batch",
                "input audit is not canonically bound to exact effects",
            ));
        }
        groups
            .entry((
                audit.operation,
                audit.reason.clone(),
                audit.factual_owner_id.clone(),
                audit.actor_subject_id.clone(),
                memory_space_id,
                audit.created_at,
            ))
            .or_default()
            .extend(audit.effects.iter().cloned());
    }

    let mut bound = Vec::with_capacity(groups.len());
    for ((operation, reason, owner, actor, memory_space_id, created_at), effects) in groups {
        let mut audit = LongTermMemoryControlAuditEvent::new(
            "pending",
            transaction_id,
            operation,
            effects,
            reason,
            owner,
            actor,
            memory_space_id,
            created_at,
        );
        audit.bind_canonical_event_id()?;
        bound.push(audit);
    }
    bound.sort_by(|left, right| left.event_id.cmp(&right.event_id));
    Ok(bound)
}

fn canonical_long_term_tombstone_id(
    operation: LongTermControlOperation,
    predecessor: &LongTermMemoryVersionMaterial,
    control_revision_id: &str,
    effective_at: u64,
) -> Result<String> {
    let encoded = serde_json::to_vec(&(
        operation,
        predecessor.owner_revision_ref(),
        &predecessor.content_digest,
        control_revision_id,
        effective_at,
    ))
    .map_err(|error| Error::config("long_term_control_tombstone", error.to_string()))?;
    let mut hasher = Sha256::new();
    hash_control_field(&mut hasher, b"long_term_control_tombstone_v3");
    hash_control_field(&mut hasher, &encoded);
    Ok(format!("ltmt-{:x}", hasher.finalize()))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LongTermControlPostImageClosure {
    pub transaction_id: String,
    pub operation: LongTermControlOperation,
    pub memory_space_id: String,
    pub factual_owner_id: MemorySpaceId,
    pub actor_subject_id: Option<SubjectId>,
    pub owner_records: Vec<LongTermMemoryVersionMaterialImage>,
    pub revisions: Vec<GovernedDocumentImage<LongTermMemoryControlRevision>>,
    pub tombstones: Vec<GovernedDocumentImage<LongTermMemoryTombstone>>,
    pub policies: Vec<GovernedDocumentImage<MemoryLongTermGovernancePolicy>>,
    pub audits: Vec<GovernedDocumentImage<LongTermMemoryControlAuditEvent>>,
}

pub fn validate_long_term_control_post_image(
    closure: &LongTermControlPostImageClosure,
) -> GovernedPostImageValidation {
    let memory_space_id = closure.memory_space_id.trim();
    let factual_owner_id = closure.factual_owner_id.trim();
    let mut failures = Vec::new();
    if memory_space_id.is_empty()
        || factual_owner_id.is_empty()
        || closure.transaction_id.trim().is_empty()
    {
        failures.push("long_term_control_transaction_scope_invalid".to_string());
        return GovernedPostImageValidation::from_failures(failures);
    }

    let mut owners = BTreeMap::new();
    for image in &closure.owner_records {
        let logical_id = image
            .observed_owner_ref()
            .map(|owner_ref| owner_ref.owner_id.as_str())
            .unwrap_or_default();
        if !image.has_exact_physical_closure(memory_space_id, factual_owner_id) {
            failures.push("long_term_control_owner_physical_key_drift".to_string());
        }
        if image.before != image.after
            && !revision_is_exact_successor(
                image.before.as_ref().map(|owner| owner.owner_revision),
                image.after.as_ref().map(|owner| owner.owner_revision),
            )
        {
            failures.push("long_term_control_owner_revision_successor_drift".to_string());
        }
        if owners.insert(logical_id.to_string(), image).is_some() {
            failures.push("long_term_control_owner_duplicate".to_string());
        }
    }

    let mut expected_effects = Vec::new();
    for image in &closure.revisions {
        let Some(revision) = image.after.as_ref() else {
            failures.push("long_term_control_revision_delete_forbidden".to_string());
            continue;
        };
        if image.before.is_some() {
            failures.push("long_term_control_revision_append_only_violation".to_string());
        }
        validate_control_physical_key(
            image,
            memory_space_id,
            LONG_TERM_CONTROL_REVISION_NAMESPACE,
            &revision.revision_id,
            "long_term_control_revision_physical_key_drift",
            &mut failures,
        );
        if revision.validate_contract().is_err() {
            failures.push("long_term_control_revision_schema_version_drift".to_string());
        }
        if revision.operation != closure.operation
            || revision.memory_space_id != memory_space_id
            || revision.factual_owner_id != factual_owner_id
            || revision.actor_subject_id != closure.actor_subject_id
        {
            failures.push("long_term_control_revision_operation_scope_drift".to_string());
        }
        let before_owner = owners
            .get(&revision.transition.predecessor.owner_ref.owner_id)
            .and_then(|owner| owner.before.as_ref());
        let after_owner = revision
            .transition
            .successor
            .as_ref()
            .and_then(|successor| {
                owners
                    .get(&successor.owner_ref.owner_id)
                    .and_then(|owner| owner.after.as_ref())
            });
        if before_owner.is_none_or(|owner| {
            owner.owner_revision_ref() != revision.transition.predecessor
                || owner.content_digest != revision.predecessor_material_digest
        }) || match (
            revision.transition.successor.as_ref(),
            revision.successor_material_digest.as_ref(),
            after_owner,
        ) {
            (Some(successor), Some(successor_digest), Some(owner)) => {
                owner.owner_revision_ref() != *successor
                    || owner.content_digest != *successor_digest
            }
            (None, None, None) => false,
            _ => true,
        } {
            failures.push("long_term_control_revision_owner_version_or_digest_drift".to_string());
        }
        expected_effects.push(ControlEffectRef::Revision {
            revision_id: revision.revision_id.clone(),
            transition: revision.transition.clone(),
            factual_owner_id: revision.factual_owner_id.clone(),
        });
    }

    for image in &closure.tombstones {
        let Some(tombstone) = image.after.as_ref() else {
            failures.push("long_term_control_tombstone_delete_forbidden".to_string());
            continue;
        };
        if image.before.is_some() {
            failures.push("long_term_control_tombstone_append_only_violation".to_string());
        }
        validate_control_physical_key(
            image,
            memory_space_id,
            LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
            &tombstone.record_id,
            "long_term_control_tombstone_physical_key_drift",
            &mut failures,
        );
        if tombstone.validate_contract().is_err() {
            failures.push("long_term_control_tombstone_schema_version_drift".to_string());
        }
        if tombstone.operation != closure.operation
            || tombstone.memory_space_id != memory_space_id
            || tombstone.factual_owner_id != factual_owner_id
            || tombstone.actor_subject_id != closure.actor_subject_id
        {
            failures.push("long_term_control_tombstone_operation_scope_drift".to_string());
        }
        match owners
            .get(&tombstone.record_id)
            .and_then(|image| image.before.as_ref())
        {
            Some(owner)
                if owner.owner_revision == tombstone.last_owner_revision
                    && owner.governed_content.source_revision == tombstone.last_source_revision
                    && owner.content_digest == tombstone.previous_digest
                    && owner.subject_visibility == tombstone.subject_visibility => {}
            _ => failures
                .push("long_term_control_tombstone_owner_version_or_digest_drift".to_string()),
        }
        expected_effects.push(ControlEffectRef::Tombstone {
            tombstone_id: tombstone.tombstone_id.clone(),
            record_id: tombstone.record_id.clone(),
            factual_owner_id: tombstone.factual_owner_id.clone(),
            owner_revision: tombstone.last_owner_revision,
            source_revision: tombstone.last_source_revision,
        });
    }

    for image in &closure.policies {
        let policy = image.after.as_ref().or(image.before.as_ref());
        let Some(policy) = policy else {
            failures.push("long_term_control_policy_image_empty".to_string());
            continue;
        };
        validate_control_physical_key(
            image,
            memory_space_id,
            LONG_TERM_GOVERNANCE_POLICY_NAMESPACE,
            &policy.policy_id,
            "long_term_control_policy_physical_key_drift",
            &mut failures,
        );
        if policy.schema_version != LONG_TERM_CONTROL_SCHEMA_VERSION
            || policy.memory_space_id != memory_space_id
            || policy.selector.memory_space_id.as_deref() != Some(memory_space_id)
        {
            failures.push("long_term_control_policy_schema_or_scope_drift".to_string());
        }
        let before_revision = image.before.as_ref().map(|value| value.policy_revision);
        let after_revision = image.after.as_ref().map(|value| value.policy_revision);
        let revision_valid = match (before_revision, after_revision) {
            (None, Some(1)) => true,
            (Some(before), Some(after)) => before.checked_add(1) == Some(after),
            (Some(_), None) => true,
            _ => false,
        };
        if !revision_valid {
            failures.push("long_term_control_policy_revision_successor_drift".to_string());
        }
        expected_effects.push(ControlEffectRef::Policy {
            policy_id: policy.policy_id.clone(),
            factual_owner_id: factual_owner_id.to_string(),
            policy_revision: image
                .after
                .as_ref()
                .map(|value| value.policy_revision)
                .unwrap_or(policy.policy_revision),
            deleted: image.after.is_none(),
        });
    }
    expected_effects.sort();
    if expected_effects.windows(2).any(|pair| pair[0] == pair[1]) {
        failures.push("long_term_control_effect_duplicate".to_string());
    }

    if closure.audits.len() != 1 {
        failures.push("long_term_control_exactly_one_audit_required".to_string());
    }
    for image in &closure.audits {
        let Some(audit) = image.after.as_ref() else {
            failures.push("long_term_control_audit_missing".to_string());
            continue;
        };
        if image.before.is_some() {
            failures.push("long_term_control_audit_append_only_violation".to_string());
        }
        validate_control_physical_key(
            image,
            memory_space_id,
            LONG_TERM_CONTROL_AUDIT_NAMESPACE,
            &audit.event_id,
            "long_term_control_audit_physical_key_drift",
            &mut failures,
        );
        if audit.schema_version != LONG_TERM_CONTROL_SCHEMA_VERSION {
            failures.push("long_term_control_audit_schema_version_drift".to_string());
        }
        if audit.transaction_id != closure.transaction_id {
            failures.push("long_term_control_audit_transaction_drift".to_string());
        }
        if audit.operation != closure.operation {
            failures.push("long_term_control_audit_operation_drift".to_string());
        }
        if audit.memory_space_id.as_deref() != Some(memory_space_id)
            || audit.factual_owner_id != factual_owner_id
        {
            failures.push("long_term_control_audit_scope_drift".to_string());
        }
        if audit.actor_subject_id != closure.actor_subject_id {
            failures.push("long_term_control_audit_actor_drift".to_string());
        }
        let mut actual_effects = audit.effects.clone();
        actual_effects.sort();
        let unique = actual_effects.iter().collect::<BTreeSet<_>>();
        if unique.len() != actual_effects.len() || actual_effects != expected_effects {
            failures.push("long_term_control_audit_effect_exact_closure_drift".to_string());
        }
    }

    GovernedPostImageValidation::from_failures(failures)
}

fn validate_control_physical_key<T>(
    image: &GovernedDocumentImage<T>,
    memory_space_id: &str,
    namespace: &str,
    logical_id: &str,
    failure: &str,
    failures: &mut Vec<String>,
) {
    if scoped_long_term_control_storage_key(memory_space_id, namespace, logical_id)
        .map(|expected| image.physical_key != expected)
        .unwrap_or(true)
    {
        failures.push(failure.to_string());
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LongTermMemoryOwnerWrite {
    Put(Box<LongTermMemoryEntry>),
    Delete { record_id: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LongTermMemoryControlWrite {
    PutRevisionIntent(LongTermMemoryControlRevisionIntent),
    PutTombstone(LongTermMemoryTombstone),
    PutGovernancePolicy(MemoryLongTermGovernancePolicy),
    DeleteGovernancePolicy {
        policy_id: String,
        factual_owner_id: MemorySpaceId,
        policy_revision: u64,
    },
    AppendAudit(LongTermMemoryControlAuditEvent),
}

fn control_effects_from_writes(
    writes: &[LongTermMemoryControlWrite],
) -> Result<Vec<ControlEffectRef>> {
    let mut effects = writes
        .iter()
        .map(|write| {
            Ok(match write {
                LongTermMemoryControlWrite::PutRevisionIntent(revision) => {
                    Some(ControlEffectRef::Revision {
                        revision_id: revision.revision_id.clone(),
                        transition: revision.transition.clone(),
                        factual_owner_id: revision.factual_owner_id.clone(),
                    })
                }
                LongTermMemoryControlWrite::PutTombstone(tombstone) => {
                    Some(ControlEffectRef::Tombstone {
                        tombstone_id: tombstone.tombstone_id.clone(),
                        record_id: tombstone.record_id.clone(),
                        factual_owner_id: tombstone.factual_owner_id.clone(),
                        owner_revision: tombstone.last_owner_revision,
                        source_revision: tombstone.last_source_revision,
                    })
                }
                LongTermMemoryControlWrite::PutGovernancePolicy(policy) => {
                    Some(ControlEffectRef::Policy {
                        policy_id: policy.policy_id.clone(),
                        factual_owner_id: required_policy_factual_owner(&policy.selector)?,
                        policy_revision: policy.policy_revision,
                        deleted: false,
                    })
                }
                LongTermMemoryControlWrite::DeleteGovernancePolicy {
                    policy_id,
                    factual_owner_id,
                    policy_revision,
                } => Some(ControlEffectRef::Policy {
                    policy_id: policy_id.clone(),
                    factual_owner_id: factual_owner_id.clone(),
                    policy_revision: *policy_revision,
                    deleted: true,
                }),
                LongTermMemoryControlWrite::AppendAudit(_) => None,
            })
        })
        .collect::<Result<Vec<Option<_>>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    effects.sort();
    effects.dedup();
    Ok(effects)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LongTermMemoryControlMutationPlan {
    pub report: MemoryLongTermMutationReport,
    pub owner_writes: Vec<LongTermMemoryOwnerWrite>,
    pub control_writes: Vec<LongTermMemoryControlWrite>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LongTermMemoryGovernancePolicyMutationPlan {
    pub report: MemoryGovernancePolicyMutationReport,
    pub control_writes: Vec<LongTermMemoryControlWrite>,
}

pub trait LongTermMemoryControlReadStore: Send + Sync {
    fn list_long_term_control_revisions(
        &self,
        record_id: &str,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryControlRevision>>;
    fn get_long_term_control_tombstone(
        &self,
        record_id: &str,
    ) -> Result<Option<LongTermMemoryTombstone>>;
    fn list_long_term_control_tombstones(
        &self,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryTombstone>>;
    fn list_long_term_governance_policies(
        &self,
        limit: usize,
    ) -> Result<Vec<MemoryLongTermGovernancePolicy>>;
    fn list_long_term_control_audit(
        &self,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryControlAuditEvent>>;
}

pub(crate) trait LongTermMemoryControlStore: LongTermMemoryControlReadStore {
    fn put_long_term_control_revision_intent(
        &self,
        revision: &LongTermMemoryControlRevisionIntent,
    ) -> Result<()>;
    fn put_long_term_control_tombstone(&self, tombstone: &LongTermMemoryTombstone) -> Result<()>;
    fn put_long_term_governance_policy(
        &self,
        policy: &MemoryLongTermGovernancePolicy,
    ) -> Result<()>;
    fn delete_long_term_governance_policy(&self, policy_id: &str) -> Result<bool>;
    fn put_long_term_control_audit(&self, event: &LongTermMemoryControlAuditEvent) -> Result<()>;
    fn pending_long_term_control_revision_intents(
        &self,
        _record_id: &str,
    ) -> Vec<LongTermMemoryControlRevisionIntent> {
        Vec::new()
    }
}

struct PlanningLongTermMemoryStore<'a> {
    inner: &'a dyn LongTermMemoryReadStore,
    overlay: Mutex<BTreeMap<String, Option<LongTermMemoryEntry>>>,
    writes: Mutex<Vec<LongTermMemoryOwnerWrite>>,
}

impl<'a> PlanningLongTermMemoryStore<'a> {
    fn new(inner: &'a dyn LongTermMemoryReadStore) -> Self {
        Self {
            inner,
            overlay: Mutex::new(BTreeMap::new()),
            writes: Mutex::new(Vec::new()),
        }
    }

    fn into_writes(self) -> Vec<LongTermMemoryOwnerWrite> {
        self.writes.into_inner().expect("owner writes lock")
    }
}

impl LongTermMemoryStore for PlanningLongTermMemoryStore<'_> {
    fn upsert_many(&self, drafts: &[LongTermMemoryDraft], now_secs: u64) -> Result<usize> {
        let mut changed = 0usize;
        for draft in drafts {
            let Some(record_id) = draft.stable_id() else {
                continue;
            };
            let previous = LongTermMemoryStore::get(self, &record_id)?;
            match plan_long_term_memory_upsert(previous.as_ref(), draft, now_secs) {
                LongTermMemoryEntryPlan::Created(entry)
                | LongTermMemoryEntryPlan::Updated(entry) => {
                    self.overlay
                        .lock()
                        .expect("owner overlay lock")
                        .insert(record_id, Some(entry.clone()));
                    self.writes
                        .lock()
                        .expect("owner writes lock")
                        .push(LongTermMemoryOwnerWrite::Put(Box::new(entry)));
                    changed = changed.saturating_add(1);
                }
                LongTermMemoryEntryPlan::Noop => {}
                LongTermMemoryEntryPlan::Rejected(reason) => {
                    return Err(Error::config(
                        "long_term_control_plan_upsert",
                        format!("owner upsert rejected: {reason:?}"),
                    ));
                }
            }
        }
        Ok(changed)
    }

    fn create_exact_owner(&self, entry: &LongTermMemoryEntry) -> Result<()> {
        if LongTermMemoryStore::get(self, &entry.id)?.is_some() {
            return Err(Error::config(
                "long_term_control_plan_exact_create",
                "exact owner creation requires an absent owner id",
            ));
        }
        self.overlay
            .lock()
            .expect("owner overlay lock")
            .insert(entry.id.clone(), Some(entry.clone()));
        self.writes
            .lock()
            .expect("owner writes lock")
            .push(LongTermMemoryOwnerWrite::Put(Box::new(entry.clone())));
        Ok(())
    }

    fn recall(
        &self,
        query: &str,
        source_chat_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryEntry>> {
        self.inner.recall(query, source_chat_id, limit)
    }

    fn get(&self, id: &str) -> Result<Option<LongTermMemoryEntry>> {
        if let Some(entry) = self
            .overlay
            .lock()
            .expect("owner overlay lock")
            .get(id)
            .cloned()
        {
            return Ok(entry);
        }
        self.inner.get(id)
    }

    fn mutate_owner(
        &self,
        id: &str,
        mutation: &LongTermMemoryOwnerMutation,
        now_secs: u64,
    ) -> Result<LongTermMemoryEntryPlan> {
        let Some(previous) = LongTermMemoryStore::get(self, id)? else {
            return Err(Error::config(
                "long_term_control_plan_owner_mutation",
                format!("owner record not found: {id}"),
            ));
        };
        let plan = plan_long_term_memory_owner_mutation(&previous, mutation, now_secs);
        if let LongTermMemoryEntryPlan::Updated(entry) | LongTermMemoryEntryPlan::Created(entry) =
            &plan
        {
            self.overlay
                .lock()
                .expect("owner overlay lock")
                .insert(id.to_string(), Some(entry.clone()));
            self.writes
                .lock()
                .expect("owner writes lock")
                .push(LongTermMemoryOwnerWrite::Put(Box::new(entry.clone())));
        }
        Ok(plan)
    }

    fn list(&self, limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
        let mut entries = self
            .inner
            .list(usize::MAX)?
            .into_iter()
            .map(|entry| (entry.id.clone(), entry))
            .collect::<BTreeMap<_, _>>();
        for (record_id, entry) in self.overlay.lock().expect("owner overlay lock").iter() {
            match entry {
                Some(entry) => {
                    entries.insert(record_id.clone(), entry.clone());
                }
                None => {
                    entries.remove(record_id);
                }
            }
        }
        let mut entries = entries.into_values().collect::<Vec<_>>();
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.updated_at));
        entries.truncate(limit);
        Ok(entries)
    }

    fn delete(&self, id: &str) -> Result<bool> {
        if LongTermMemoryStore::get(self, id)?.is_none() {
            return Ok(false);
        }
        self.overlay
            .lock()
            .expect("owner overlay lock")
            .insert(id.to_string(), None);
        self.writes
            .lock()
            .expect("owner writes lock")
            .push(LongTermMemoryOwnerWrite::Delete {
                record_id: id.to_string(),
            });
        Ok(true)
    }

    fn delete_slot(&self, slot: &LongTermMemorySlot) -> Result<bool> {
        let Some(id) = slot.stable_id() else {
            return Ok(false);
        };
        self.delete(&id)
    }

    fn count(&self) -> Result<usize> {
        Ok(LongTermMemoryStore::list(self, usize::MAX)?.len())
    }
}

struct PlanningLongTermMemoryControlStore<'a> {
    inner: &'a dyn LongTermMemoryControlReadStore,
    revision_intents: Mutex<Vec<LongTermMemoryControlRevisionIntent>>,
    tombstones: Mutex<BTreeMap<String, LongTermMemoryTombstone>>,
    policies: Mutex<BTreeMap<String, Option<MemoryLongTermGovernancePolicy>>>,
    audits: Mutex<Vec<LongTermMemoryControlAuditEvent>>,
    writes: Mutex<Vec<LongTermMemoryControlWrite>>,
}

impl<'a> PlanningLongTermMemoryControlStore<'a> {
    fn new(inner: &'a dyn LongTermMemoryControlReadStore) -> Self {
        Self {
            inner,
            revision_intents: Mutex::new(Vec::new()),
            tombstones: Mutex::new(BTreeMap::new()),
            policies: Mutex::new(BTreeMap::new()),
            audits: Mutex::new(Vec::new()),
            writes: Mutex::new(Vec::new()),
        }
    }

    fn into_writes(self) -> Vec<LongTermMemoryControlWrite> {
        self.writes.into_inner().expect("control writes lock")
    }
}

impl LongTermMemoryControlReadStore for PlanningLongTermMemoryControlStore<'_> {
    fn list_long_term_control_revisions(
        &self,
        record_id: &str,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryControlRevision>> {
        let mut revisions = self
            .inner
            .list_long_term_control_revisions(record_id, limit)?;
        revisions.sort_by_key(|revision| std::cmp::Reverse(revision.created_at));
        revisions.truncate(limit);
        Ok(revisions)
    }

    fn get_long_term_control_tombstone(
        &self,
        record_id: &str,
    ) -> Result<Option<LongTermMemoryTombstone>> {
        if let Some(tombstone) = self
            .tombstones
            .lock()
            .expect("control tombstones lock")
            .get(record_id)
            .cloned()
        {
            return Ok(Some(tombstone));
        }
        self.inner.get_long_term_control_tombstone(record_id)
    }

    fn list_long_term_control_tombstones(
        &self,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryTombstone>> {
        let mut tombstones = self
            .inner
            .list_long_term_control_tombstones(usize::MAX)?
            .into_iter()
            .map(|tombstone| (tombstone.record_id.clone(), tombstone))
            .collect::<BTreeMap<_, _>>();
        tombstones.extend(
            self.tombstones
                .lock()
                .expect("control tombstones lock")
                .clone(),
        );
        let mut tombstones = tombstones.into_values().collect::<Vec<_>>();
        tombstones.sort_by_key(|tombstone| std::cmp::Reverse(tombstone.created_at));
        tombstones.truncate(limit);
        Ok(tombstones)
    }

    fn list_long_term_governance_policies(
        &self,
        limit: usize,
    ) -> Result<Vec<MemoryLongTermGovernancePolicy>> {
        let mut policies = self
            .inner
            .list_long_term_governance_policies(usize::MAX)?
            .into_iter()
            .map(|policy| (policy.policy_id.clone(), policy))
            .collect::<BTreeMap<_, _>>();
        for (policy_id, policy) in self.policies.lock().expect("control policies lock").iter() {
            match policy {
                Some(policy) => {
                    policies.insert(policy_id.clone(), policy.clone());
                }
                None => {
                    policies.remove(policy_id);
                }
            }
        }
        let mut policies = policies.into_values().collect::<Vec<_>>();
        policies.sort_by_key(|policy| std::cmp::Reverse(policy.updated_at));
        policies.truncate(limit);
        Ok(policies)
    }

    fn list_long_term_control_audit(
        &self,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryControlAuditEvent>> {
        let mut audits = self.inner.list_long_term_control_audit(usize::MAX)?;
        audits.extend(self.audits.lock().expect("control audits lock").clone());
        audits.sort_by_key(|audit| std::cmp::Reverse(audit.created_at));
        audits.truncate(limit);
        Ok(audits)
    }
}

impl LongTermMemoryControlStore for PlanningLongTermMemoryControlStore<'_> {
    fn put_long_term_control_revision_intent(
        &self,
        revision: &LongTermMemoryControlRevisionIntent,
    ) -> Result<()> {
        self.revision_intents
            .lock()
            .expect("control revision intents lock")
            .push(revision.clone());
        self.writes.lock().expect("control writes lock").push(
            LongTermMemoryControlWrite::PutRevisionIntent(revision.clone()),
        );
        Ok(())
    }

    fn put_long_term_control_tombstone(&self, tombstone: &LongTermMemoryTombstone) -> Result<()> {
        self.tombstones
            .lock()
            .expect("control tombstones lock")
            .insert(tombstone.record_id.clone(), tombstone.clone());
        self.writes
            .lock()
            .expect("control writes lock")
            .push(LongTermMemoryControlWrite::PutTombstone(tombstone.clone()));
        Ok(())
    }

    fn put_long_term_governance_policy(
        &self,
        policy: &MemoryLongTermGovernancePolicy,
    ) -> Result<()> {
        self.policies
            .lock()
            .expect("control policies lock")
            .insert(policy.policy_id.clone(), Some(policy.clone()));
        self.writes.lock().expect("control writes lock").push(
            LongTermMemoryControlWrite::PutGovernancePolicy(policy.clone()),
        );
        Ok(())
    }

    fn delete_long_term_governance_policy(&self, policy_id: &str) -> Result<bool> {
        let existing = self
            .list_long_term_governance_policies(usize::MAX)?
            .into_iter()
            .find(|policy| policy.policy_id == policy_id);
        if let Some(existing) = existing {
            self.policies
                .lock()
                .expect("control policies lock")
                .insert(policy_id.to_string(), None);
            self.writes.lock().expect("control writes lock").push(
                LongTermMemoryControlWrite::DeleteGovernancePolicy {
                    policy_id: policy_id.to_string(),
                    factual_owner_id: required_policy_factual_owner(&existing.selector)?,
                    policy_revision: existing.policy_revision,
                },
            );
            return Ok(true);
        }
        Ok(false)
    }

    fn put_long_term_control_audit(&self, event: &LongTermMemoryControlAuditEvent) -> Result<()> {
        let mut event = event.clone();
        event.effects =
            control_effects_from_writes(&self.writes.lock().expect("control writes lock"))?;
        self.audits
            .lock()
            .expect("control audits lock")
            .push(event.clone());
        self.writes
            .lock()
            .expect("control writes lock")
            .push(LongTermMemoryControlWrite::AppendAudit(event));
        Ok(())
    }

    fn pending_long_term_control_revision_intents(
        &self,
        record_id: &str,
    ) -> Vec<LongTermMemoryControlRevisionIntent> {
        self.revision_intents
            .lock()
            .expect("control revision intents lock")
            .iter()
            .filter(|intent| intent.transition.predecessor.owner_ref.owner_id == record_id)
            .cloned()
            .collect()
    }
}

pub fn list_long_term_memory_control_page(
    store: &dyn LongTermMemoryReadStore,
    control_store: &dyn LongTermMemoryControlReadStore,
    request: LongTermMemoryControlListRequest,
) -> Result<MemoryLongTermListReport> {
    let mut records = store.query(&request.query)?;
    records.retain(|entry| !is_tombstoned(control_store, &entry.id));
    let total_visible = records.len();
    let start = request
        .cursor
        .as_deref()
        .and_then(|cursor| records.iter().position(|entry| entry.id == cursor))
        .map(|index| index.saturating_add(1))
        .unwrap_or(0);
    let limit = request.limit.clamp(1, super::MAX_LONG_TERM_MEMORY_ITEMS);
    let next_cursor = records
        .get(start.saturating_add(limit))
        .map(|entry| entry.id.clone());
    let records = records
        .into_iter()
        .skip(start)
        .take(limit)
        .map(|record| record_report(control_store, record, request.view))
        .collect::<Result<Vec<_>>>()?;
    Ok(MemoryLongTermListReport {
        records,
        next_cursor,
        total_visible,
        view: request.view,
    })
}

pub fn get_long_term_memory_control_detail(
    store: &dyn LongTermMemoryReadStore,
    control_store: &dyn LongTermMemoryControlReadStore,
    request: LongTermMemoryControlDetailRequest,
) -> Result<MemoryLongTermDetailReport> {
    let requested_record_id = match &request.target {
        MemoryLongTermTarget::RecordId(record_id) => Some(record_id.clone()),
        _ => None,
    };
    let resolved = resolve_target(store, control_store, &request.target, false)?;
    let raw_record = resolved.records.first().cloned();
    let record_id = raw_record
        .as_ref()
        .map(|entry| entry.id.clone())
        .or(requested_record_id);
    let revisions = match record_id.as_deref() {
        Some(record_id) => control_store.list_long_term_control_revisions(record_id, 64)?,
        None => Vec::new(),
    };
    let tombstone = match record_id.as_deref() {
        Some(record_id) => control_store.get_long_term_control_tombstone(record_id)?,
        None => None,
    };
    let transcript_refs = raw_record
        .as_ref()
        .map(extract_transcript_refs)
        .unwrap_or_default();
    let record = raw_record.map(|record| long_term_record_for_view(record, request.view));
    Ok(MemoryLongTermDetailReport {
        record,
        revisions,
        tombstone,
        transcript_refs,
        view: request.view,
    })
}

pub fn plan_long_term_memory_control_mutation(
    store: &dyn LongTermMemoryReadStore,
    control_store: &dyn LongTermMemoryControlReadStore,
    request: LongTermMemoryControlMutationRequest,
) -> Result<LongTermMemoryControlMutationPlan> {
    let planning_store = PlanningLongTermMemoryStore::new(store);
    let planning_control_store = PlanningLongTermMemoryControlStore::new(control_store);
    let report = plan_long_term_memory_control_mutation_with_sinks(
        &planning_store,
        &planning_control_store,
        request,
    )?;
    Ok(LongTermMemoryControlMutationPlan {
        report,
        owner_writes: planning_store.into_writes(),
        control_writes: planning_control_store.into_writes(),
    })
}

#[cfg(any(test, feature = "nonproduction-replay-harness"))]
pub(crate) fn apply_long_term_memory_control_mutation(
    store: &dyn LongTermMemoryStore,
    control_store: &dyn LongTermMemoryControlStore,
    request: LongTermMemoryControlMutationRequest,
) -> Result<MemoryLongTermMutationReport> {
    plan_long_term_memory_control_mutation_with_sinks(store, control_store, request)
}

fn plan_long_term_memory_control_mutation_with_sinks(
    store: &dyn LongTermMemoryStore,
    control_store: &dyn LongTermMemoryControlStore,
    request: LongTermMemoryControlMutationRequest,
) -> Result<MemoryLongTermMutationReport> {
    validate_factual_owner_scope(&request)?;
    let operation_label = mutation_label(&request.operation);
    match &request.operation {
        MemoryLongTermMutation::Correct {
            target,
            replacement,
        } => apply_correct(store, control_store, &request, target, replacement),
        MemoryLongTermMutation::Supersede {
            target,
            replacement,
        } => apply_supersede(store, control_store, &request, target, replacement),
        MemoryLongTermMutation::Invalidate { contract } => {
            apply_invalidate(store, control_store, &request, contract)
        }
        MemoryLongTermMutation::Delete { target } => {
            apply_delete(store, control_store, &request, target, operation_label)
        }
        MemoryLongTermMutation::ForgetByQuery {
            selector,
            confirmation_token,
        } => apply_forget_by_query(
            store,
            control_store,
            &request,
            selector,
            confirmation_token.as_deref(),
        ),
        MemoryLongTermMutation::MarkStale { target, stale_hint } => {
            apply_mark_stale(store, control_store, &request, target, *stale_hint)
        }
        MemoryLongTermMutation::ChangeScope {
            target,
            source_scope,
            subject_visibility,
        } => apply_change_scope(
            store,
            control_store,
            &request,
            target,
            *source_scope,
            subject_visibility.clone(),
        ),
        MemoryLongTermMutation::ChangePrivacy { target, privacy } => {
            apply_change_privacy(store, control_store, &request, target, *privacy)
        }
    }
}

pub fn plan_long_term_memory_governance_policy_mutation(
    control_store: &dyn LongTermMemoryControlReadStore,
    operation: MemoryGovernancePolicyMutation,
    reason: String,
    dry_run: bool,
    now_secs: u64,
) -> Result<LongTermMemoryGovernancePolicyMutationPlan> {
    let planning_control_store = PlanningLongTermMemoryControlStore::new(control_store);
    let report = plan_long_term_memory_governance_policy_mutation_with_sink(
        &planning_control_store,
        operation,
        reason,
        dry_run,
        now_secs,
    )?;
    Ok(LongTermMemoryGovernancePolicyMutationPlan {
        report,
        control_writes: planning_control_store.into_writes(),
    })
}

#[cfg(any(test, feature = "nonproduction-replay-harness"))]
pub(crate) fn apply_long_term_memory_governance_policy_mutation(
    control_store: &dyn LongTermMemoryControlStore,
    operation: MemoryGovernancePolicyMutation,
    reason: String,
    dry_run: bool,
    now_secs: u64,
) -> Result<MemoryGovernancePolicyMutationReport> {
    plan_long_term_memory_governance_policy_mutation_with_sink(
        control_store,
        operation,
        reason,
        dry_run,
        now_secs,
    )
}

fn plan_long_term_memory_governance_policy_mutation_with_sink(
    control_store: &dyn LongTermMemoryControlStore,
    operation: MemoryGovernancePolicyMutation,
    reason: String,
    dry_run: bool,
    now_secs: u64,
) -> Result<MemoryGovernancePolicyMutationReport> {
    let selector = match &operation {
        MemoryGovernancePolicyMutation::Suppress { selector, .. }
        | MemoryGovernancePolicyMutation::Pause { selector, .. }
        | MemoryGovernancePolicyMutation::Resume { selector }
        | MemoryGovernancePolicyMutation::RemoveSuppression { selector } => selector,
    };
    required_policy_memory_space(selector)?;
    required_policy_factual_owner(selector)?;
    match operation {
        MemoryGovernancePolicyMutation::Suppress { selector, duration } => {
            let memory_space_id = required_policy_memory_space(&selector)?;
            let policy_id = stable_id("ltmp", &("suppress", &selector, &duration));
            let previous = control_store
                .list_long_term_governance_policies(usize::MAX)?
                .into_iter()
                .find(|policy| policy.policy_id == policy_id);
            let policy = MemoryLongTermGovernancePolicy {
                schema_version: LONG_TERM_CONTROL_SCHEMA_VERSION,
                policy_revision: previous
                    .as_ref()
                    .map(|policy| policy.policy_revision.saturating_add(1).max(1))
                    .unwrap_or(1),
                memory_space_id,
                policy_id: policy_id.clone(),
                kind: "suppress".to_string(),
                selector,
                duration: Some(duration),
                expires_at: None,
                reason: reason.clone(),
                created_at: previous
                    .as_ref()
                    .map(|policy| policy.created_at)
                    .unwrap_or(now_secs),
                updated_at: now_secs,
            };
            if !dry_run {
                control_store.put_long_term_governance_policy(&policy)?;
            }
            let audit_event_id = write_policy_audit(
                control_store,
                "policy.suppress",
                std::slice::from_ref(&policy),
                false,
                &reason,
                now_secs,
                dry_run,
            )?;
            Ok(MemoryGovernancePolicyMutationReport {
                accepted: !dry_run,
                dry_run,
                operation: "policy.suppress",
                policy_id: Some(policy_id),
                affected_future_writes: "suppressed".to_string(),
                policy_decision: MemoryLongTermControlDecision {
                    accepted: !dry_run,
                    reason: "suppression_policy_registered".to_string(),
                    confirmation_token: None,
                },
                audit_event_id,
                reason,
            })
        }
        MemoryGovernancePolicyMutation::Pause {
            selector,
            expires_at,
        } => {
            let memory_space_id = required_policy_memory_space(&selector)?;
            let policy_id = stable_id("ltmp", &("pause", &selector, expires_at));
            let previous = control_store
                .list_long_term_governance_policies(usize::MAX)?
                .into_iter()
                .find(|policy| policy.policy_id == policy_id);
            let policy = MemoryLongTermGovernancePolicy {
                schema_version: LONG_TERM_CONTROL_SCHEMA_VERSION,
                policy_revision: previous
                    .as_ref()
                    .map(|policy| policy.policy_revision.saturating_add(1).max(1))
                    .unwrap_or(1),
                memory_space_id,
                policy_id: policy_id.clone(),
                kind: "pause".to_string(),
                selector,
                duration: None,
                expires_at,
                reason: reason.clone(),
                created_at: previous
                    .as_ref()
                    .map(|policy| policy.created_at)
                    .unwrap_or(now_secs),
                updated_at: now_secs,
            };
            if !dry_run {
                control_store.put_long_term_governance_policy(&policy)?;
            }
            let audit_event_id = write_policy_audit(
                control_store,
                "policy.pause",
                std::slice::from_ref(&policy),
                false,
                &reason,
                now_secs,
                dry_run,
            )?;
            Ok(MemoryGovernancePolicyMutationReport {
                accepted: !dry_run,
                dry_run,
                operation: "policy.pause",
                policy_id: Some(policy_id),
                affected_future_writes: "paused".to_string(),
                policy_decision: MemoryLongTermControlDecision {
                    accepted: !dry_run,
                    reason: "pause_policy_registered".to_string(),
                    confirmation_token: None,
                },
                audit_event_id,
                reason,
            })
        }
        MemoryGovernancePolicyMutation::Resume { selector } => {
            remove_policies_by_selector_and_kind(
                control_store,
                selector,
                "pause",
                "policy.resume",
                reason,
                dry_run,
                now_secs,
            )
        }
        MemoryGovernancePolicyMutation::RemoveSuppression { selector } => {
            remove_policies_by_selector_and_kind(
                control_store,
                selector,
                "suppress",
                "policy.remove_suppression",
                reason,
                dry_run,
                now_secs,
            )
        }
    }
}

fn remove_policies_by_selector_and_kind(
    control_store: &dyn LongTermMemoryControlStore,
    selector: MemoryGovernanceSelector,
    policy_kind: &str,
    operation: &'static str,
    reason: String,
    dry_run: bool,
    now_secs: u64,
) -> Result<MemoryGovernancePolicyMutationReport> {
    let policies = control_store.list_long_term_governance_policies(usize::MAX)?;
    let matched = policies
        .into_iter()
        .filter(|policy| policy.kind == policy_kind && policy.selector == selector)
        .collect::<Vec<_>>();
    if !dry_run {
        for policy in &matched {
            control_store.delete_long_term_governance_policy(&policy.policy_id)?;
        }
    }
    let policy_id = matched.first().map(|policy| policy.policy_id.clone());
    let audit_event_id = if matched.is_empty() {
        None
    } else {
        write_policy_audit(
            control_store,
            operation,
            &matched,
            true,
            &reason,
            now_secs,
            dry_run,
        )?
    };
    Ok(MemoryGovernancePolicyMutationReport {
        accepted: !dry_run,
        dry_run,
        operation,
        policy_id,
        affected_future_writes: "policy_removed".to_string(),
        policy_decision: MemoryLongTermControlDecision {
            accepted: !dry_run,
            reason: format!("matched_policy_count={}", matched.len()),
            confirmation_token: None,
        },
        audit_event_id,
        reason,
    })
}

fn apply_correct(
    store: &dyn LongTermMemoryStore,
    control_store: &dyn LongTermMemoryControlStore,
    request: &LongTermMemoryControlMutationRequest,
    target: &MemoryLongTermTarget,
    replacement: &LongTermMemoryDraft,
) -> Result<MemoryLongTermMutationReport> {
    let resolved = resolve_target(store, control_store, target, false)?;
    let Some(previous) = resolved.records.first().cloned() else {
        return Ok(rejected_report(
            "correct",
            request,
            resolved.report,
            "target_not_found",
            None,
        ));
    };
    let Some(replacement_id) = replacement.stable_id() else {
        return Err(Error::config(
            "long_term_control_correct",
            "replacement has no stable id",
        ));
    };
    if replacement_id != previous.id {
        return Ok(rejected_report(
            "correct",
            request,
            resolved.report,
            "correct_requires_same_record_lineage_use_supersede",
            None,
        ));
    }
    if replacement.privacy != previous.privacy {
        return Ok(rejected_report(
            "correct",
            request,
            resolved.report,
            "privacy_transition_requires_change_privacy",
            None,
        ));
    }
    let previous_digest = digest_entry(&previous)?;
    let mutation = LongTermMemoryOwnerMutation::Correct(replacement.clone());
    let plan = plan_or_apply_owner_mutation(
        store,
        &previous,
        &mutation,
        request.now_secs,
        request.dry_run,
    )?;
    let updated = match plan {
        LongTermMemoryEntryPlan::Updated(entry) => entry,
        LongTermMemoryEntryPlan::Noop => {
            return Ok(accepted_record_report(
                "correct",
                request,
                resolved,
                Vec::new(),
                Vec::new(),
                None,
                MemoryProjectionImpactReport {
                    affected_record_ids: Vec::new(),
                    subject_visibility: previous.subject_visibility.clone(),
                    recall_projection_must_refresh: false,
                    notes: vec!["noop".to_string()],
                },
            ))
        }
        LongTermMemoryEntryPlan::Rejected(reason) => {
            return Ok(rejected_report(
                "correct",
                request,
                resolved.report,
                format!("owner_mutation_rejected:{reason:?}"),
                None,
            ))
        }
        LongTermMemoryEntryPlan::Created(_) => {
            return Err(Error::config(
                "long_term_control_correct",
                "owner mutation unexpectedly created a record",
            ))
        }
    };
    let new_digest = digest_entry(&updated)?;
    if !request.dry_run {
        let revision = control_revision_intent_for(
            LongTermControlOperation::Correct,
            &previous,
            Some(&updated),
            request,
            Vec::new(),
        )?;
        control_store.put_long_term_control_revision_intent(&revision)?;
    }
    let audit_event_id = write_record_audit(
        control_store,
        "correct",
        std::slice::from_ref(&previous.id),
        std::slice::from_ref(&updated),
        request,
        request.dry_run,
    )?;
    Ok(accepted_record_report(
        "correct",
        request,
        resolved,
        vec![MemoryLongTermAffectedRecord {
            record_id: previous.id.clone(),
            operation: "correct".to_string(),
            previous_owner_revision: previous.owner_revision,
            new_owner_revision: Some(updated.owner_revision),
            previous_source_revision: previous.source_revision,
            new_source_revision: updated.source_revision,
            previous_digest,
            new_digest: Some(new_digest),
        }],
        Vec::new(),
        audit_event_id,
        MemoryProjectionImpactReport {
            affected_record_ids: vec![previous.id.clone()],
            subject_visibility: updated.subject_visibility.clone(),
            recall_projection_must_refresh: true,
            notes: Vec::new(),
        },
    ))
}

fn apply_supersede(
    store: &dyn LongTermMemoryStore,
    control_store: &dyn LongTermMemoryControlStore,
    request: &LongTermMemoryControlMutationRequest,
    target: &MemoryLongTermTarget,
    replacement: &LongTermMemoryDraft,
) -> Result<MemoryLongTermMutationReport> {
    let resolved = resolve_target(store, control_store, target, false)?;
    let Some(previous) = resolved.records.first().cloned() else {
        return Ok(rejected_report(
            "supersede",
            request,
            resolved.report,
            "target_not_found",
            None,
        ));
    };
    let Some(new_id) = replacement.stable_id() else {
        return Err(Error::config(
            "long_term_control_supersede",
            "replacement has no stable id",
        ));
    };
    if new_id == previous.id {
        return Ok(rejected_report(
            "supersede",
            request,
            resolved.report,
            "supersede_requires_new_record_lineage_use_correct",
            None,
        ));
    }
    if store.get(&new_id)?.is_some() {
        return Ok(rejected_report(
            "supersede",
            request,
            resolved.report,
            "replacement_owner_already_exists",
            None,
        ));
    }
    let replacement_plan = plan_long_term_memory_upsert(None, replacement, request.now_secs);
    let mut replacement_entry = match replacement_plan {
        LongTermMemoryEntryPlan::Created(entry) => entry,
        LongTermMemoryEntryPlan::Rejected(reason) => {
            return Ok(rejected_report(
                "supersede",
                request,
                resolved.report,
                format!("replacement_rejected:{reason:?}"),
                None,
            ))
        }
        LongTermMemoryEntryPlan::Updated(_) | LongTermMemoryEntryPlan::Noop => {
            return Err(Error::config(
                "long_term_control_supersede",
                "absent replacement owner did not produce an exact creation",
            ))
        }
    };
    replacement_entry.subject_visibility = previous.subject_visibility.clone();
    let previous_digest = digest_entry(&previous)?;
    let new_digest = digest_entry(&replacement_entry)?;
    let tombstone = tombstone_for(&previous, LongTermControlOperation::Supersede, request)?;
    if !request.dry_run {
        control_store.put_long_term_control_tombstone(&tombstone)?;
        store.delete(&previous.id)?;
        store.create_exact_owner(&replacement_entry)?;
        let revision = control_revision_intent_for(
            LongTermControlOperation::Supersede,
            &previous,
            Some(&replacement_entry),
            request,
            Vec::new(),
        )?;
        control_store.put_long_term_control_revision_intent(&revision)?;
    }
    let audit_event_id = write_record_audit(
        control_store,
        "supersede",
        &[previous.id.clone(), new_id.clone()],
        &[previous.clone(), replacement_entry.clone()],
        request,
        request.dry_run,
    )?;
    Ok(accepted_record_report(
        "supersede",
        request,
        resolved,
        vec![MemoryLongTermAffectedRecord {
            record_id: previous.id.clone(),
            operation: "supersede".to_string(),
            previous_owner_revision: previous.owner_revision,
            new_owner_revision: None,
            previous_source_revision: previous.source_revision,
            new_source_revision: None,
            previous_digest,
            new_digest: Some(new_digest),
        }],
        vec![MemoryLongTermTombstoneRef {
            record_id: previous.id.clone(),
            tombstone_id: tombstone.tombstone_id,
            operation: LongTermControlOperation::Supersede,
            last_owner_revision: previous.owner_revision,
            last_source_revision: previous.source_revision,
        }],
        audit_event_id,
        MemoryProjectionImpactReport {
            affected_record_ids: vec![previous.id.clone(), new_id],
            subject_visibility: replacement_entry.subject_visibility.clone(),
            recall_projection_must_refresh: true,
            notes: Vec::new(),
        },
    ))
}

fn apply_invalidate(
    store: &dyn LongTermMemoryStore,
    control_store: &dyn LongTermMemoryControlStore,
    request: &LongTermMemoryControlMutationRequest,
    contract: &LongTermInvalidationContract,
) -> Result<MemoryLongTermMutationReport> {
    let validation = contract.validate_contract();
    if !validation.accepted {
        return Err(Error::config(
            "long_term_control_invalidation",
            format!("invalidation contract rejected: {:?}", validation.failures),
        ));
    }
    if request.reason != contract.audit_reason
        || request.actor_subject_id.as_deref() != Some(contract.actor_subject_id.as_str())
    {
        return Err(Error::config(
            "long_term_control_invalidation",
            "request reason and actor must exactly match the typed invalidation contract",
        ));
    }
    let resolved = resolve_target(store, control_store, &contract.target, false)?;
    let Some(previous) = resolved.records.first().cloned() else {
        return Ok(rejected_report(
            "invalidate",
            request,
            resolved.report,
            "target_not_found",
            None,
        ));
    };
    let previous_digest = digest_entry(&previous)?;
    if !request.dry_run {
        let memory_space_id = request.memory_space_id.clone().ok_or_else(|| {
            Error::config(
                "long_term_control_memory_space_required",
                "invalidation requires an explicit memory_space_id",
            )
        })?;
        let revision = LongTermMemoryControlRevisionIntent::for_invalidation(
            stable_id(
                "ltmr",
                &(
                    LongTermControlOperation::Invalidate.as_str(),
                    &previous.id,
                    previous.owner_revision,
                    request.now_secs,
                ),
            ),
            &previous,
            contract.reason_code,
            contract.audit_reason.clone(),
            request.factual_owner_id.clone(),
            contract.actor_subject_id.clone(),
            memory_space_id,
            request.now_secs,
            contract.governed_evidence_refs.clone(),
        )?;
        control_store.put_long_term_control_revision_intent(&revision)?;
    }
    let audit_event_id = write_record_audit(
        control_store,
        "invalidate",
        std::slice::from_ref(&previous.id),
        std::slice::from_ref(&previous),
        request,
        request.dry_run,
    )?;
    Ok(accepted_record_report(
        "invalidate",
        request,
        resolved,
        vec![MemoryLongTermAffectedRecord {
            record_id: previous.id.clone(),
            operation: "invalidate".to_string(),
            previous_owner_revision: previous.owner_revision,
            new_owner_revision: None,
            previous_source_revision: previous.source_revision,
            new_source_revision: None,
            previous_digest,
            new_digest: None,
        }],
        Vec::new(),
        audit_event_id,
        MemoryProjectionImpactReport {
            affected_record_ids: vec![previous.id],
            subject_visibility: MemorySubjectVisibilityPolicy::AllSubjects,
            recall_projection_must_refresh: true,
            notes: vec!["retained_operator_only".to_string()],
        },
    ))
}

fn apply_delete(
    store: &dyn LongTermMemoryStore,
    control_store: &dyn LongTermMemoryControlStore,
    request: &LongTermMemoryControlMutationRequest,
    target: &MemoryLongTermTarget,
    operation_label: &'static str,
) -> Result<MemoryLongTermMutationReport> {
    let resolved = resolve_target(store, control_store, target, false)?;
    let Some(previous) = resolved.records.first().cloned() else {
        return Ok(rejected_report(
            operation_label,
            request,
            resolved.report,
            "target_not_found",
            None,
        ));
    };
    let previous_digest = digest_entry(&previous)?;
    let operation = LongTermControlOperation::from_label(operation_label).ok_or_else(|| {
        Error::config(
            "long_term_control_revision_intent",
            format!("unknown terminal operation {operation_label}"),
        )
    })?;
    let tombstone = tombstone_for(&previous, operation, request)?;
    if !request.dry_run {
        let revision =
            control_revision_intent_for(operation, &previous, None, request, Vec::new())?;
        control_store.put_long_term_control_revision_intent(&revision)?;
        control_store.put_long_term_control_tombstone(&tombstone)?;
        store.delete(&previous.id)?;
    }
    let audit_event_id = write_record_audit(
        control_store,
        operation_label,
        std::slice::from_ref(&previous.id),
        std::slice::from_ref(&previous),
        request,
        request.dry_run,
    )?;
    Ok(accepted_record_report(
        operation_label,
        request,
        resolved,
        vec![MemoryLongTermAffectedRecord {
            record_id: previous.id.clone(),
            operation: operation_label.to_string(),
            previous_owner_revision: previous.owner_revision,
            new_owner_revision: None,
            previous_source_revision: previous.source_revision,
            new_source_revision: None,
            previous_digest,
            new_digest: None,
        }],
        vec![MemoryLongTermTombstoneRef {
            record_id: previous.id.clone(),
            tombstone_id: tombstone.tombstone_id,
            operation,
            last_owner_revision: previous.owner_revision,
            last_source_revision: previous.source_revision,
        }],
        audit_event_id,
        MemoryProjectionImpactReport {
            affected_record_ids: vec![previous.id.clone()],
            subject_visibility: MemorySubjectVisibilityPolicy::AllSubjects,
            recall_projection_must_refresh: true,
            notes: Vec::new(),
        },
    ))
}

fn apply_forget_by_query(
    store: &dyn LongTermMemoryStore,
    control_store: &dyn LongTermMemoryControlStore,
    request: &LongTermMemoryControlMutationRequest,
    selector: &MemoryLongTermSelector,
    confirmation_token: Option<&str>,
) -> Result<MemoryLongTermMutationReport> {
    let target = MemoryLongTermTarget::Query(selector.clone());
    let resolved = resolve_target(store, control_store, &target, true)?;
    let required_token = confirmation_token_for(selector, &resolved.record_ids);
    if request.dry_run {
        return Ok(rejected_report(
            "forget_by_query",
            request,
            resolved.report,
            "preview_only_confirmation_required",
            Some(required_token),
        ));
    }
    if confirmation_token != Some(required_token.as_str()) {
        return Ok(rejected_report(
            "forget_by_query",
            request,
            resolved.report,
            "confirmation_required",
            Some(required_token),
        ));
    }
    let mut affected = Vec::new();
    let mut tombstones = Vec::new();
    for previous in &resolved.records {
        let previous_digest = digest_entry(previous)?;
        let tombstone = tombstone_for(previous, LongTermControlOperation::ForgetByQuery, request)?;
        let revision = control_revision_intent_for(
            LongTermControlOperation::ForgetByQuery,
            previous,
            None,
            request,
            Vec::new(),
        )?;
        control_store.put_long_term_control_revision_intent(&revision)?;
        control_store.put_long_term_control_tombstone(&tombstone)?;
        store.delete(&previous.id)?;
        affected.push(MemoryLongTermAffectedRecord {
            record_id: previous.id.clone(),
            operation: "forget_by_query".to_string(),
            previous_owner_revision: previous.owner_revision,
            new_owner_revision: None,
            previous_source_revision: previous.source_revision,
            new_source_revision: None,
            previous_digest,
            new_digest: None,
        });
        tombstones.push(MemoryLongTermTombstoneRef {
            record_id: previous.id.clone(),
            tombstone_id: tombstone.tombstone_id,
            operation: LongTermControlOperation::ForgetByQuery,
            last_owner_revision: previous.owner_revision,
            last_source_revision: previous.source_revision,
        });
    }
    let audit_event_id = write_record_audit(
        control_store,
        "forget_by_query",
        &resolved.record_ids,
        &resolved.records,
        request,
        false,
    )?;
    Ok(accepted_record_report(
        "forget_by_query",
        request,
        resolved,
        affected,
        tombstones,
        audit_event_id,
        MemoryProjectionImpactReport {
            affected_record_ids: Vec::new(),
            subject_visibility: MemorySubjectVisibilityPolicy::AllSubjects,
            recall_projection_must_refresh: true,
            notes: Vec::new(),
        },
    ))
}

fn apply_mark_stale(
    store: &dyn LongTermMemoryStore,
    control_store: &dyn LongTermMemoryControlStore,
    request: &LongTermMemoryControlMutationRequest,
    target: &MemoryLongTermTarget,
    stale_hint: LongTermMemoryStaleHint,
) -> Result<MemoryLongTermMutationReport> {
    let resolved = resolve_target(store, control_store, target, false)?;
    let Some(previous) = resolved.records.first().cloned() else {
        return Ok(rejected_report(
            "mark_stale",
            request,
            resolved.report,
            "target_not_found",
            None,
        ));
    };
    apply_owner_field_mutation(
        "mark_stale",
        store,
        control_store,
        request,
        resolved,
        previous,
        LongTermMemoryOwnerMutation::MarkStale(stale_hint),
        Vec::new(),
    )
}

fn apply_change_scope(
    store: &dyn LongTermMemoryStore,
    control_store: &dyn LongTermMemoryControlStore,
    request: &LongTermMemoryControlMutationRequest,
    target: &MemoryLongTermTarget,
    source_scope: LongTermMemorySourceScope,
    subject_visibility: MemorySubjectVisibilityPolicy,
) -> Result<MemoryLongTermMutationReport> {
    let resolved = resolve_target(store, control_store, target, false)?;
    let Some(previous) = resolved.records.first().cloned() else {
        return Ok(rejected_report(
            "change_scope",
            request,
            resolved.report,
            "target_not_found",
            None,
        ));
    };
    if subject_visibility.validate_canonical().is_err() {
        return Ok(rejected_report(
            "change_scope",
            request,
            resolved.report,
            "subject_visibility_policy_invalid",
            None,
        ));
    }
    apply_owner_field_mutation(
        "change_scope",
        store,
        control_store,
        request,
        resolved,
        previous,
        LongTermMemoryOwnerMutation::ChangeScope {
            source_scope,
            subject_visibility,
        },
        Vec::new(),
    )
}

fn apply_change_privacy(
    store: &dyn LongTermMemoryStore,
    control_store: &dyn LongTermMemoryControlStore,
    request: &LongTermMemoryControlMutationRequest,
    target: &MemoryLongTermTarget,
    privacy: MemoryPrivacyClass,
) -> Result<MemoryLongTermMutationReport> {
    let resolved = resolve_target(store, control_store, target, false)?;
    let Some(previous) = resolved.records.first().cloned() else {
        return Ok(rejected_report(
            "change_privacy",
            request,
            resolved.report,
            "target_not_found",
            None,
        ));
    };
    apply_owner_field_mutation(
        "change_privacy",
        store,
        control_store,
        request,
        resolved,
        previous,
        LongTermMemoryOwnerMutation::ChangePrivacy(privacy),
        vec!["privacy_transition_requires_facet_refresh".to_string()],
    )
}

#[allow(clippy::too_many_arguments)]
fn apply_owner_field_mutation(
    operation: &'static str,
    store: &dyn LongTermMemoryStore,
    control_store: &dyn LongTermMemoryControlStore,
    request: &LongTermMemoryControlMutationRequest,
    resolved: ResolvedTarget,
    previous: LongTermMemoryEntry,
    mutation: LongTermMemoryOwnerMutation,
    notes: Vec<String>,
) -> Result<MemoryLongTermMutationReport> {
    let previous_digest = digest_entry(&previous)?;
    let plan = plan_or_apply_owner_mutation(
        store,
        &previous,
        &mutation,
        request.now_secs,
        request.dry_run,
    )?;
    let updated = match plan {
        LongTermMemoryEntryPlan::Updated(entry) => entry,
        LongTermMemoryEntryPlan::Noop => {
            return Ok(accepted_record_report(
                operation,
                request,
                resolved,
                Vec::new(),
                Vec::new(),
                None,
                MemoryProjectionImpactReport {
                    affected_record_ids: Vec::new(),
                    subject_visibility: previous.subject_visibility.clone(),
                    recall_projection_must_refresh: false,
                    notes: vec!["noop".to_string()],
                },
            ))
        }
        LongTermMemoryEntryPlan::Rejected(reason) => {
            return Ok(rejected_report(
                operation,
                request,
                resolved.report,
                format!("owner_mutation_rejected:{reason:?}"),
                None,
            ))
        }
        LongTermMemoryEntryPlan::Created(_) => {
            return Err(Error::config(
                "long_term_control_owner_mutation",
                "owner mutation unexpectedly created a record",
            ))
        }
    };
    let new_digest = digest_entry(&updated)?;
    if !request.dry_run {
        let typed_operation = LongTermControlOperation::from_label(operation).ok_or_else(|| {
            Error::config(
                "long_term_control_revision_intent",
                format!("unknown owner mutation operation {operation}"),
            )
        })?;
        let revision = control_revision_intent_for(
            typed_operation,
            &previous,
            Some(&updated),
            request,
            Vec::new(),
        )?;
        control_store.put_long_term_control_revision_intent(&revision)?;
    }
    let audit_event_id = write_record_audit(
        control_store,
        operation,
        std::slice::from_ref(&previous.id),
        std::slice::from_ref(&updated),
        request,
        request.dry_run,
    )?;
    Ok(accepted_record_report(
        operation,
        request,
        resolved,
        vec![MemoryLongTermAffectedRecord {
            record_id: previous.id.clone(),
            operation: operation.to_string(),
            previous_owner_revision: previous.owner_revision,
            new_owner_revision: Some(updated.owner_revision),
            previous_source_revision: previous.source_revision,
            new_source_revision: updated.source_revision,
            previous_digest,
            new_digest: Some(new_digest),
        }],
        Vec::new(),
        audit_event_id,
        MemoryProjectionImpactReport {
            affected_record_ids: vec![previous.id],
            subject_visibility: updated.subject_visibility,
            recall_projection_must_refresh: true,
            notes,
        },
    ))
}

struct ResolvedTarget {
    report: MemoryLongTermTargetResolutionReport,
    records: Vec<LongTermMemoryEntry>,
    record_ids: Vec<String>,
}

fn resolve_target<S>(
    store: &S,
    control_store: &dyn LongTermMemoryControlReadStore,
    target: &MemoryLongTermTarget,
    allow_query: bool,
) -> Result<ResolvedTarget>
where
    S: LongTermMemoryReadStore + ?Sized,
{
    let mut records = match target {
        MemoryLongTermTarget::RecordId(id) => store.get(id)?.into_iter().collect::<Vec<_>>(),
        MemoryLongTermTarget::Slot(slot) => store.get_slot(slot)?.into_iter().collect::<Vec<_>>(),
        MemoryLongTermTarget::TranscriptDerivedRef(reference) => {
            if !matches!(
                reference.plane,
                DerivedMemoryPlane::LongTerm | DerivedMemoryPlane::SharedFact
            ) {
                Vec::new()
            } else {
                let record_id = long_term_record_id_from_derived_ref(reference);
                store.get(&record_id)?.into_iter().collect::<Vec<_>>()
            }
        }
        MemoryLongTermTarget::Query(selector) if allow_query => {
            let mut records = store.query(&selector.query)?;
            if let Some(evidence_ref) = selector.evidence_ref.as_ref() {
                records.retain(|entry| entry_matches_transcript_ref(entry, evidence_ref));
            }
            records
        }
        MemoryLongTermTarget::Query(_) => {
            return Err(Error::config(
                "long_term_control_target",
                "query target requires explicit bulk operation",
            ));
        }
    };
    records.retain(|entry| !is_tombstoned(control_store, &entry.id));
    let record_ids = records
        .iter()
        .map(|entry| entry.id.clone())
        .collect::<Vec<_>>();
    let report = MemoryLongTermTargetResolutionReport {
        resolved_count: records.len(),
        ambiguous_count: usize::from(!allow_query && records.len() > 1),
        not_found_count: usize::from(records.is_empty()),
        resolved_record_ids: record_ids.clone(),
    };
    Ok(ResolvedTarget {
        report,
        records,
        record_ids,
    })
}

fn long_term_record_id_from_derived_ref(reference: &DerivedMemoryRef) -> String {
    let key = reference.store_key.trim();
    key.strip_prefix("long_term:")
        .or_else(|| key.strip_prefix("shared_fact:"))
        .unwrap_or(key)
        .trim()
        .to_string()
}

fn record_report(
    control_store: &dyn LongTermMemoryControlReadStore,
    record: LongTermMemoryEntry,
    view: MemoryLongTermControlView,
) -> Result<MemoryLongTermRecordReport> {
    let evidence = long_term_memory_evidence_summary(&record, crate::util::current_unix_secs());
    let tombstoned = control_store
        .get_long_term_control_tombstone(&record.id)?
        .is_some();
    let transcript_refs = extract_transcript_refs(&record);
    let mut evidence_summary = evidence.summary;
    if !transcript_refs.is_empty() {
        match view {
            MemoryLongTermControlView::HostUi => {
                evidence_summary.push_str("; transcript evidence refs: ");
                evidence_summary.push_str(&transcript_refs.len().to_string());
            }
            MemoryLongTermControlView::Operator | MemoryLongTermControlView::RawOwner => {
                let citations = transcript_refs
                    .iter()
                    .map(TranscriptEvidenceRef::display_citation)
                    .collect::<Vec<_>>()
                    .join(", ");
                evidence_summary.push_str("; transcript evidence: ");
                evidence_summary.push_str(&citations);
            }
        }
    }
    let record = long_term_record_for_view(record, view);
    Ok(MemoryLongTermRecordReport {
        transcript_refs,
        record,
        evidence_summary,
        tombstoned,
    })
}

fn extract_transcript_refs(entry: &LongTermMemoryEntry) -> Vec<TranscriptEvidenceRef> {
    entry
        .supporting_citations
        .iter()
        .filter_map(|citation| TranscriptEvidenceRef::parse_display_citation(citation))
        .collect()
}

fn entry_matches_transcript_ref(
    entry: &LongTermMemoryEntry,
    expected: &TranscriptEvidenceRef,
) -> bool {
    extract_transcript_refs(entry)
        .iter()
        .any(|actual| transcript_ref_matches(actual, expected))
        || entry
            .supporting_citations
            .iter()
            .any(|citation| citation.trim() == expected.display_citation())
}

fn transcript_ref_matches(
    actual: &TranscriptEvidenceRef,
    expected: &TranscriptEvidenceRef,
) -> bool {
    actual.memory_space_id == expected.memory_space_id
        && actual.channel_id == expected.channel_id
        && actual.conversation_id == expected.conversation_id
        && actual.turn_id == expected.turn_id
        && actual.message_id == expected.message_id
}

fn long_term_record_for_view(
    mut record: LongTermMemoryEntry,
    view: MemoryLongTermControlView,
) -> LongTermMemoryEntry {
    if matches!(view, MemoryLongTermControlView::HostUi) {
        record.source_chat_id = None;
        record.supporting_citations.clear();
    }
    record
}

fn is_tombstoned(control_store: &dyn LongTermMemoryControlReadStore, record_id: &str) -> bool {
    control_store
        .get_long_term_control_tombstone(record_id)
        .ok()
        .flatten()
        .is_some()
}

fn accepted_record_report(
    operation: &'static str,
    request: &LongTermMemoryControlMutationRequest,
    resolved: ResolvedTarget,
    affected_records: Vec<MemoryLongTermAffectedRecord>,
    tombstones: Vec<MemoryLongTermTombstoneRef>,
    audit_event_id: Option<String>,
    mut projection_impact: MemoryProjectionImpactReport,
) -> MemoryLongTermMutationReport {
    if projection_impact.affected_record_ids.is_empty() {
        projection_impact.affected_record_ids = affected_records
            .iter()
            .map(|record| record.record_id.clone())
            .collect();
    }
    let transcript_refs = resolved
        .records
        .iter()
        .flat_map(extract_transcript_refs)
        .collect::<Vec<_>>();
    let evidence_refs = resolved
        .records
        .iter()
        .flat_map(|entry| {
            extract_transcript_refs(entry)
                .into_iter()
                .map(|source| DerivedMemoryRef {
                    plane: DerivedMemoryPlane::LongTerm,
                    store_key: entry.id.clone(),
                    subject_id: source.subject_id.clone(),
                    source,
                    created_at: request.now_secs,
                })
        })
        .collect::<Vec<_>>();
    MemoryLongTermMutationReport {
        accepted: !request.dry_run,
        dry_run: request.dry_run,
        operation,
        target_report: resolved.report,
        affected_records,
        affected_facet_docs: Vec::new(),
        tombstones,
        evidence_refs,
        transcript_refs,
        policy_decision: MemoryLongTermControlDecision {
            accepted: !request.dry_run,
            reason: "accepted".to_string(),
            confirmation_token: None,
        },
        projection_impact,
        deferred_governance_impact: MemoryDeferredGovernanceImpactReport::default(),
        audit_event_id,
        reason: request.reason.clone(),
    }
}

fn rejected_report(
    operation: &'static str,
    request: &LongTermMemoryControlMutationRequest,
    target_report: MemoryLongTermTargetResolutionReport,
    reason: impl Into<String>,
    confirmation_token: Option<String>,
) -> MemoryLongTermMutationReport {
    MemoryLongTermMutationReport {
        accepted: false,
        dry_run: request.dry_run,
        operation,
        target_report,
        affected_records: Vec::new(),
        affected_facet_docs: Vec::new(),
        tombstones: Vec::new(),
        evidence_refs: Vec::new(),
        transcript_refs: Vec::new(),
        policy_decision: MemoryLongTermControlDecision {
            accepted: false,
            reason: reason.into(),
            confirmation_token,
        },
        projection_impact: MemoryProjectionImpactReport::default(),
        deferred_governance_impact: MemoryDeferredGovernanceImpactReport::default(),
        audit_event_id: None,
        reason: request.reason.clone(),
    }
}

fn tombstone_for(
    entry: &LongTermMemoryEntry,
    operation: LongTermControlOperation,
    request: &LongTermMemoryControlMutationRequest,
) -> Result<LongTermMemoryTombstone> {
    entry.subject_visibility.validate_canonical()?;
    let memory_space_id = request.memory_space_id.clone().ok_or_else(|| {
        Error::config(
            "long_term_control_memory_space_required",
            "control tombstones require an explicit memory_space_id",
        )
    })?;
    let tombstone = LongTermMemoryTombstone {
        schema_version: LONG_TERM_CONTROL_TOMBSTONE_SCHEMA_VERSION,
        tombstone_id: stable_id("ltmt", &(operation.as_str(), &entry.id, request.now_secs)),
        record_id: entry.id.clone(),
        operation,
        last_owner_revision: entry.owner_revision,
        last_source_revision: entry.source_revision,
        previous_digest: digest_entry(entry)?,
        subject_visibility: entry.subject_visibility.clone(),
        reason: request.reason.clone(),
        factual_owner_id: request.factual_owner_id.clone(),
        actor_subject_id: request.actor_subject_id.clone(),
        memory_space_id,
        created_at: request.now_secs,
    };
    tombstone.validate_contract()?;
    Ok(tombstone)
}

fn control_revision_intent_for(
    operation: LongTermControlOperation,
    before: &LongTermMemoryEntry,
    after: Option<&LongTermMemoryEntry>,
    request: &LongTermMemoryControlMutationRequest,
    governed_evidence_refs: Vec<GovernedOwnerRevisionRef>,
) -> Result<LongTermMemoryControlRevisionIntent> {
    let memory_space_id = request.memory_space_id.clone().ok_or_else(|| {
        Error::config(
            "long_term_control_memory_space_required",
            "control mutations require an explicit memory_space_id",
        )
    })?;
    LongTermMemoryControlRevisionIntent::for_owner_change(
        stable_id(
            "ltmr",
            &(
                operation.as_str(),
                &before.id,
                after.map(|entry| entry.id.as_str()),
                after.map(|entry| entry.owner_revision),
                request.now_secs,
            ),
        ),
        operation,
        before,
        after,
        request.reason.clone(),
        request.factual_owner_id.clone(),
        request.actor_subject_id.clone(),
        memory_space_id,
        request.now_secs,
        governed_evidence_refs,
    )
}

fn write_record_audit(
    control_store: &dyn LongTermMemoryControlStore,
    operation: &str,
    record_ids: &[String],
    _records: &[LongTermMemoryEntry],
    request: &LongTermMemoryControlMutationRequest,
    dry_run: bool,
) -> Result<Option<String>> {
    if dry_run {
        return Ok(None);
    }
    let typed_operation = LongTermControlOperation::from_label(operation).ok_or_else(|| {
        Error::config(
            "long_term_control_audit_operation",
            format!("unknown control operation: {operation}"),
        )
    })?;
    let memory_space_id = request.memory_space_id.as_deref().ok_or_else(|| {
        Error::config(
            "long_term_control_memory_space_required",
            "control mutations require an explicit memory_space_id",
        )
    })?;
    let mut effects = Vec::new();
    for record_id in record_ids {
        effects.extend(
            control_store
                .pending_long_term_control_revision_intents(record_id)
                .into_iter()
                .filter(|revision| {
                    revision.operation == typed_operation
                        && revision.created_at == request.now_secs
                        && revision.memory_space_id == memory_space_id
                        && revision.factual_owner_id == request.factual_owner_id
                })
                .map(|revision| ControlEffectRef::Revision {
                    revision_id: revision.revision_id,
                    transition: revision.transition,
                    factual_owner_id: revision.factual_owner_id,
                }),
        );
        if let Some(tombstone) = control_store.get_long_term_control_tombstone(record_id)? {
            if tombstone.operation == typed_operation
                && tombstone.created_at == request.now_secs
                && tombstone.memory_space_id == memory_space_id
                && tombstone.factual_owner_id == request.factual_owner_id
            {
                effects.push(ControlEffectRef::Tombstone {
                    tombstone_id: tombstone.tombstone_id,
                    record_id: tombstone.record_id,
                    factual_owner_id: tombstone.factual_owner_id,
                    owner_revision: tombstone.last_owner_revision,
                    source_revision: tombstone.last_source_revision,
                });
            }
        }
    }
    if effects.is_empty() {
        return Err(Error::config(
            "long_term_control_audit_effects",
            "accepted control mutation produced no auditable effects",
        ));
    }
    let mut audit = LongTermMemoryControlAuditEvent::new(
        "pending",
        "pending",
        typed_operation,
        effects,
        request.reason.clone(),
        request.factual_owner_id.clone(),
        request.actor_subject_id.clone(),
        memory_space_id,
        request.now_secs,
    );
    audit.bind_canonical_event_id()?;
    audit.transaction_id = audit.event_id.clone();
    let event_id = audit.event_id.clone();
    control_store.put_long_term_control_audit(&audit)?;
    Ok(Some(event_id))
}

fn write_policy_audit(
    control_store: &dyn LongTermMemoryControlStore,
    operation: &str,
    policies: &[MemoryLongTermGovernancePolicy],
    deleted: bool,
    reason: &str,
    now_secs: u64,
    dry_run: bool,
) -> Result<Option<String>> {
    if dry_run {
        return Ok(None);
    }
    let typed_operation = LongTermControlOperation::from_label(operation).ok_or_else(|| {
        Error::config(
            "long_term_control_audit_operation",
            format!("unknown control operation: {operation}"),
        )
    })?;
    let memory_space_id = policies
        .first()
        .map(|policy| policy.memory_space_id.as_str())
        .ok_or_else(|| Error::config("long_term_control_policy_audit", "policy set is empty"))?;
    if policies
        .iter()
        .any(|policy| policy.memory_space_id != memory_space_id)
    {
        return Err(Error::config(
            "long_term_control_policy_audit_scope",
            "one policy audit cannot span memory spaces",
        ));
    }
    let factual_owner_id = required_policy_factual_owner(&policies[0].selector)?;
    for policy in policies {
        if required_policy_factual_owner(&policy.selector)? != factual_owner_id {
            return Err(Error::config(
                "long_term_control_policy_audit_scope",
                "one policy audit cannot span factual owners",
            ));
        }
    }
    let effects = policies
        .iter()
        .map(|policy| ControlEffectRef::Policy {
            policy_id: policy.policy_id.clone(),
            factual_owner_id: factual_owner_id.clone(),
            policy_revision: policy.policy_revision,
            deleted,
        })
        .collect();
    let mut audit = LongTermMemoryControlAuditEvent::new(
        "pending",
        "pending",
        typed_operation,
        effects,
        reason,
        factual_owner_id,
        None,
        memory_space_id,
        now_secs,
    );
    audit.bind_canonical_event_id()?;
    audit.transaction_id = audit.event_id.clone();
    let event_id = audit.event_id.clone();
    control_store.put_long_term_control_audit(&audit)?;
    Ok(Some(event_id))
}

fn required_policy_memory_space(selector: &MemoryGovernanceSelector) -> Result<String> {
    selector
        .memory_space_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            Error::config(
                "long_term_control_policy_memory_space_required",
                "governance policies require an explicit memory_space_id",
            )
        })
}

fn required_policy_factual_owner(selector: &MemoryGovernanceSelector) -> Result<MemorySpaceId> {
    selector
        .subject_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            Error::config(
                "long_term_control_policy_subject_required",
                "scoped runtime governance policies require an exact subject selector",
            )
        })?;
    required_policy_memory_space(selector)
}

fn validate_factual_owner_scope(request: &LongTermMemoryControlMutationRequest) -> Result<()> {
    let factual_owner_id = request.factual_owner_id.trim();
    let memory_space_id = request
        .memory_space_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            Error::config(
                "long_term_control_memory_space_required",
                "control mutations require an explicit memory_space_id",
            )
        })?;
    if factual_owner_id.is_empty()
        || factual_owner_id != request.factual_owner_id
        || factual_owner_id != memory_space_id
    {
        return Err(Error::config(
            "long_term_control_factual_owner_scope",
            "shared long-term factual owner must exactly equal memory_space_id",
        ));
    }
    Ok(())
}

fn confirmation_token_for(selector: &MemoryLongTermSelector, record_ids: &[String]) -> String {
    stable_text_id(
        "ltmc",
        &format!(
            "{}:{:?}",
            serde_json::to_string(selector).unwrap_or_default(),
            record_ids
        ),
    )
}

fn selector_matches_candidate(
    selector: &MemoryGovernanceSelector,
    memory_space_id: Option<&str>,
    subject_id: Option<&str>,
    kind: &LongTermMemoryKind,
    topic: &str,
    source_chat_id: Option<&str>,
    source_scope: LongTermMemorySourceScope,
) -> bool {
    if selector
        .memory_space_id
        .as_deref()
        .is_some_and(|expected| Some(expected) != memory_space_id)
    {
        return false;
    }
    if selector
        .subject_id
        .as_deref()
        .is_some_and(|expected| Some(expected) != subject_id)
    {
        return false;
    }
    if selector
        .kind
        .as_ref()
        .is_some_and(|expected| expected != kind)
    {
        return false;
    }
    if selector
        .source_chat_id
        .as_deref()
        .is_some_and(|expected| Some(expected) != source_chat_id)
    {
        return false;
    }
    if selector
        .source_scope
        .is_some_and(|expected| expected != source_scope)
    {
        return false;
    }
    selector
        .topic_pattern
        .as_deref()
        .map(|pattern| wildcard_match(pattern, topic))
        .unwrap_or(true)
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.trim().to_lowercase();
    let value = value.trim().to_lowercase();
    if wildcard_match_normalized(&pattern, &value) {
        return true;
    }
    let pattern = normalize_wildcard_pattern(&pattern);
    let value = normalize_policy_match_value(&value);
    wildcard_match_normalized(&pattern, &value)
}

fn wildcard_match_normalized(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return value.starts_with(prefix);
    }
    if let Some(suffix) = pattern.strip_prefix('*') {
        return value.ends_with(suffix);
    }
    pattern == value
}

fn normalize_wildcard_pattern(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut prev_sep = false;
    for ch in value.chars() {
        if ch == '*' {
            out.push('*');
            prev_sep = false;
        } else if ch.is_alphanumeric() || is_cjk(ch) {
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
            prev_sep = false;
        } else if !prev_sep && !out.is_empty() && !out.ends_with('*') {
            out.push('_');
            prev_sep = true;
        }
    }
    out.trim_matches('_').to_string()
}

fn normalize_policy_match_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut prev_sep = false;
    for ch in value.chars() {
        if ch.is_alphanumeric() || is_cjk(ch) {
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
            prev_sep = false;
        } else if !prev_sep && !out.is_empty() {
            out.push('_');
            prev_sep = true;
        }
    }
    out.trim_matches('_').to_string()
}

fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x4E00..=0x9FFF
            | 0x3400..=0x4DBF
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2B73F
            | 0x2B740..=0x2B81F
            | 0x2B820..=0x2CEAF
            | 0xF900..=0xFAFF
    )
}

fn mutation_label(operation: &MemoryLongTermMutation) -> &'static str {
    match operation {
        MemoryLongTermMutation::Correct { .. } => "correct",
        MemoryLongTermMutation::Supersede { .. } => "supersede",
        MemoryLongTermMutation::Invalidate { .. } => "invalidate",
        MemoryLongTermMutation::Delete { .. } => "delete",
        MemoryLongTermMutation::ForgetByQuery { .. } => "forget_by_query",
        MemoryLongTermMutation::MarkStale { .. } => "mark_stale",
        MemoryLongTermMutation::ChangeScope { .. } => "change_scope",
        MemoryLongTermMutation::ChangePrivacy { .. } => "change_privacy",
    }
}

fn digest_entry(entry: &LongTermMemoryEntry) -> Result<String> {
    let encoded = serde_json::to_vec(entry)
        .map_err(|error| Error::config("long_term_control_owner_digest", error.to_string()))?;
    let mut hasher = Sha256::new();
    hash_control_field(&mut hasher, b"canonical_long_term_owner_v3");
    hash_control_field(&mut hasher, &encoded);
    Ok(format!("{:x}", hasher.finalize()))
}

fn stable_id<T: Hash>(prefix: &str, value: &T) -> String {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{prefix}-{:016x}", hasher.finish())
}

fn stable_text_id(prefix: &str, value: &str) -> String {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{prefix}-{:016x}", hasher.finish())
}

#[cfg(test)]
mod digest_tests {
    use super::*;
    use crate::memory::{
        LongTermMemoryConfidence, LongTermMemoryFreshness, LongTermMemorySourceType,
        LongTermMemoryStaleHint,
    };

    fn owner_fixture() -> LongTermMemoryEntry {
        LongTermMemoryEntry {
            id: "ltm-owner".to_string(),
            kind: LongTermMemoryKind::Fact,
            topic: "release-contract".to_string(),
            content: "The release requires governed evidence.".to_string(),
            keywords: vec!["release".to_string(), "governance".to_string()],
            privacy: MemoryPrivacyClass::SharedWithSubject,
            source_chat_id: Some("conversation-a".to_string()),
            source_type: LongTermMemorySourceType::Conversation,
            source_scope: LongTermMemorySourceScope::User,
            subject_visibility: MemorySubjectVisibilityPolicy::AllSubjects,
            confidence: LongTermMemoryConfidence::High,
            freshness: LongTermMemoryFreshness::Dynamic,
            stale_hint: LongTermMemoryStaleHint::ReviewBeforeUse,
            supporting_citations: vec!["transcript:turn-1".to_string()],
            canonical_entities: Vec::new(),
            evidence_count: 1,
            created_at: 10,
            updated_at: 20,
            observed_at: 15,
            last_confirmed_at: 20,
            source_revision: Some(7),
            owner_revision: 1,
            last_used_at: 0,
        }
    }

    #[test]
    fn canonical_owner_digest_changes_when_only_privacy_changes() {
        let baseline = owner_fixture();
        let mut changed = baseline.clone();
        changed.privacy = MemoryPrivacyClass::PublicRuntime;

        assert_ne!(
            digest_entry(&baseline).expect("baseline digest"),
            digest_entry(&changed).expect("changed digest")
        );
    }

    #[test]
    fn canonical_owner_digest_covers_governed_owner_contract() {
        let baseline = owner_fixture();
        let baseline_digest = digest_entry(&baseline).expect("baseline digest");
        let mut variants = Vec::new();

        let mut changed = baseline.clone();
        changed.content.push_str(" Updated.");
        variants.push(("content", changed));

        let mut changed = baseline.clone();
        changed.source_scope = LongTermMemorySourceScope::World;
        variants.push(("source_scope", changed));

        let mut changed = baseline.clone();
        changed.subject_visibility =
            MemorySubjectVisibilityPolicy::OnlySubjects(vec!["agent-a".to_string()]);
        variants.push(("subject_visibility", changed));

        let mut changed = baseline.clone();
        changed.supporting_citations = vec!["transcript:turn-2".to_string()];
        variants.push(("supporting_citations", changed));

        let mut changed = baseline.clone();
        changed.source_revision = changed.source_revision.map(|revision| revision + 1);
        variants.push(("source_revision", changed));

        let mut changed = baseline.clone();
        changed.source_type = LongTermMemorySourceType::ExternalObservation;
        variants.push(("source_type", changed));

        let mut changed = baseline.clone();
        changed.observed_at += 1;
        variants.push(("observed_at", changed));

        let mut changed = baseline.clone();
        changed.last_confirmed_at += 1;
        variants.push(("last_confirmed_at", changed));

        for (field, changed) in variants {
            assert_ne!(
                baseline_digest,
                digest_entry(&changed).expect("changed digest"),
                "governed owner field missing from digest: {field}"
            );
        }
    }
}
