use serde::{Deserialize, Serialize};
use std::collections::{hash_map::DefaultHasher, BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

use crate::error::{Error, Result};

use super::governed_post_image::{
    revision_is_exact_successor, GovernedDocumentImage, GovernedPostImageValidation,
};
use super::long_term::{scoped_long_term_control_storage_key, scoped_long_term_memory_storage_key};
use super::{
    long_term_memory_evidence_summary, plan_long_term_memory_owner_mutation,
    plan_long_term_memory_upsert, DerivedMemoryPlane, DerivedMemoryRef, FacetReportView,
    LongTermMemoryDraft, LongTermMemoryEntry, LongTermMemoryEntryPlan, LongTermMemoryKind,
    LongTermMemoryOwnerMutation, LongTermMemoryQuery, LongTermMemoryReadStore, LongTermMemorySlot,
    LongTermMemorySourceScope, LongTermMemoryStaleHint, LongTermMemoryStore, MemoryFacetOwnerPlane,
    MemoryPrivacyClass, SubjectId, TranscriptEvidenceRef,
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
pub const LONG_TERM_CONTROL_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LongTermControlOperation {
    Correct,
    Supersede,
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
            Self::Correct => "correct",
            Self::Supersede => "supersede",
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
            "correct" => Some(Self::Correct),
            "supersede" => Some(Self::Supersede),
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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemorySubjectVisibilityPolicy {
    #[default]
    AllSubjects,
    OnlySubjects(Vec<SubjectId>),
    HiddenFromSubjects(Vec<SubjectId>),
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
    pub operation: String,
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
    pub facet_doc_id: String,
    pub owner_record_id: String,
    pub owner_plane: MemoryFacetOwnerPlane,
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
pub struct LongTermMemoryControlRevision {
    pub schema_version: u32,
    pub revision_id: String,
    pub record_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub successor_record_id: Option<String>,
    pub operation: String,
    pub owner_revision: u64,
    pub source_revision: Option<u64>,
    pub previous_digest: String,
    pub new_digest: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_subject_id: Option<SubjectId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_space_id: Option<String>,
    pub created_at: u64,
}

impl LongTermMemoryControlRevision {
    #[allow(clippy::too_many_arguments)]
    pub fn for_owner_change(
        revision_id: impl Into<String>,
        operation: LongTermControlOperation,
        before: &LongTermMemoryEntry,
        after: &LongTermMemoryEntry,
        reason: impl Into<String>,
        actor_subject_id: Option<SubjectId>,
        memory_space_id: impl Into<String>,
        created_at: u64,
    ) -> Self {
        Self {
            schema_version: LONG_TERM_CONTROL_SCHEMA_VERSION,
            revision_id: revision_id.into(),
            record_id: after.id.clone(),
            successor_record_id: None,
            operation: operation.as_str().to_string(),
            owner_revision: after.owner_revision,
            source_revision: after.source_revision,
            previous_digest: digest_entry(before),
            new_digest: digest_entry(after),
            reason: reason.into(),
            actor_subject_id,
            memory_space_id: Some(memory_space_id.into()),
            created_at,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LongTermMemoryTombstone {
    pub schema_version: u32,
    pub tombstone_id: String,
    pub record_id: String,
    pub operation: String,
    pub last_owner_revision: u64,
    pub last_source_revision: Option<u64>,
    pub previous_digest: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_subject_id: Option<SubjectId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_space_id: Option<String>,
    pub created_at: u64,
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
            actor_subject_id,
            memory_space_id: Some(memory_space_id.into()),
            created_at,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ControlEffectRef {
    Revision {
        revision_id: String,
        record_id: String,
        successor_record_id: Option<String>,
        owner_revision: u64,
        source_revision: Option<u64>,
    },
    Tombstone {
        tombstone_id: String,
        record_id: String,
        owner_revision: u64,
        source_revision: Option<u64>,
    },
    Policy {
        policy_id: String,
        policy_revision: u64,
        deleted: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LongTermControlPostImageClosure {
    pub transaction_id: String,
    pub operation: LongTermControlOperation,
    pub memory_space_id: String,
    pub actor_subject_id: Option<SubjectId>,
    pub owner_records: Vec<GovernedDocumentImage<LongTermMemoryEntry>>,
    pub revisions: Vec<GovernedDocumentImage<LongTermMemoryControlRevision>>,
    pub tombstones: Vec<GovernedDocumentImage<LongTermMemoryTombstone>>,
    pub policies: Vec<GovernedDocumentImage<MemoryLongTermGovernancePolicy>>,
    pub audits: Vec<GovernedDocumentImage<LongTermMemoryControlAuditEvent>>,
}

pub fn validate_long_term_control_post_image(
    closure: &LongTermControlPostImageClosure,
) -> GovernedPostImageValidation {
    let memory_space_id = closure.memory_space_id.trim();
    let mut failures = Vec::new();
    if memory_space_id.is_empty() || closure.transaction_id.trim().is_empty() {
        failures.push("long_term_control_transaction_scope_invalid".to_string());
        return GovernedPostImageValidation::from_failures(failures);
    }

    let mut owners = BTreeMap::new();
    for image in &closure.owner_records {
        let logical_id = image
            .after
            .as_ref()
            .or(image.before.as_ref())
            .map(|owner| owner.id.as_str())
            .unwrap_or_default();
        if scoped_long_term_memory_storage_key(memory_space_id, logical_id)
            .map(|expected| image.physical_key != expected)
            .unwrap_or(true)
        {
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
        if revision.schema_version != LONG_TERM_CONTROL_SCHEMA_VERSION {
            failures.push("long_term_control_revision_schema_version_drift".to_string());
        }
        if LongTermControlOperation::from_label(&revision.operation) != Some(closure.operation)
            || revision.memory_space_id.as_deref() != Some(memory_space_id)
            || revision.actor_subject_id != closure.actor_subject_id
        {
            failures.push("long_term_control_revision_operation_scope_drift".to_string());
        }
        let before_owner = owners
            .get(&revision.record_id)
            .and_then(|owner| owner.before.as_ref());
        let after_record_id = revision
            .successor_record_id
            .as_deref()
            .unwrap_or(revision.record_id.as_str());
        let after_owner = owners
            .get(after_record_id)
            .and_then(|owner| owner.after.as_ref());
        if before_owner.is_none_or(|owner| digest_entry(owner) != revision.previous_digest)
            || after_owner.is_none_or(|owner| {
                digest_entry(owner) != revision.new_digest
                    || owner.owner_revision != revision.owner_revision
                    || owner.source_revision != revision.source_revision
            })
        {
            failures.push("long_term_control_revision_owner_version_or_digest_drift".to_string());
        }
        expected_effects.push(ControlEffectRef::Revision {
            revision_id: revision.revision_id.clone(),
            record_id: revision.record_id.clone(),
            successor_record_id: revision.successor_record_id.clone(),
            owner_revision: revision.owner_revision,
            source_revision: revision.source_revision,
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
        if tombstone.schema_version != LONG_TERM_CONTROL_SCHEMA_VERSION {
            failures.push("long_term_control_tombstone_schema_version_drift".to_string());
        }
        if LongTermControlOperation::from_label(&tombstone.operation) != Some(closure.operation)
            || tombstone.memory_space_id.as_deref() != Some(memory_space_id)
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
                    && owner.source_revision == tombstone.last_source_revision
                    && digest_entry(owner) == tombstone.previous_digest => {}
            _ => failures
                .push("long_term_control_tombstone_owner_version_or_digest_drift".to_string()),
        }
        expected_effects.push(ControlEffectRef::Tombstone {
            tombstone_id: tombstone.tombstone_id.clone(),
            record_id: tombstone.record_id.clone(),
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
        if audit.transaction_id != closure.transaction_id
            || audit.operation != closure.operation
            || audit.memory_space_id.as_deref() != Some(memory_space_id)
            || audit.actor_subject_id != closure.actor_subject_id
        {
            failures.push("long_term_control_audit_transaction_operation_scope_drift".to_string());
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
pub struct LongTermMemoryControlRecordVersion {
    pub record_id: String,
    pub owner_revision: u64,
    pub source_revision: Option<u64>,
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
    PutRevision(LongTermMemoryControlRevision),
    PutTombstone(LongTermMemoryTombstone),
    PutGovernancePolicy(MemoryLongTermGovernancePolicy),
    DeleteGovernancePolicy {
        policy_id: String,
        policy_revision: u64,
    },
    AppendAudit(LongTermMemoryControlAuditEvent),
}

fn control_effects_from_writes(writes: &[LongTermMemoryControlWrite]) -> Vec<ControlEffectRef> {
    let mut effects = writes
        .iter()
        .filter_map(|write| match write {
            LongTermMemoryControlWrite::PutRevision(revision) => Some(ControlEffectRef::Revision {
                revision_id: revision.revision_id.clone(),
                record_id: revision.record_id.clone(),
                successor_record_id: revision.successor_record_id.clone(),
                owner_revision: revision.owner_revision,
                source_revision: revision.source_revision,
            }),
            LongTermMemoryControlWrite::PutTombstone(tombstone) => {
                Some(ControlEffectRef::Tombstone {
                    tombstone_id: tombstone.tombstone_id.clone(),
                    record_id: tombstone.record_id.clone(),
                    owner_revision: tombstone.last_owner_revision,
                    source_revision: tombstone.last_source_revision,
                })
            }
            LongTermMemoryControlWrite::PutGovernancePolicy(policy) => {
                Some(ControlEffectRef::Policy {
                    policy_id: policy.policy_id.clone(),
                    policy_revision: policy.policy_revision,
                    deleted: false,
                })
            }
            LongTermMemoryControlWrite::DeleteGovernancePolicy {
                policy_id,
                policy_revision,
            } => Some(ControlEffectRef::Policy {
                policy_id: policy_id.clone(),
                policy_revision: *policy_revision,
                deleted: true,
            }),
            LongTermMemoryControlWrite::AppendAudit(_) => None,
        })
        .collect::<Vec<_>>();
    effects.sort();
    effects.dedup();
    effects
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
    fn put_long_term_control_revision(
        &self,
        revision: &LongTermMemoryControlRevision,
    ) -> Result<()>;
    fn put_long_term_control_tombstone(&self, tombstone: &LongTermMemoryTombstone) -> Result<()>;
    fn put_long_term_governance_policy(
        &self,
        policy: &MemoryLongTermGovernancePolicy,
    ) -> Result<()>;
    fn delete_long_term_governance_policy(&self, policy_id: &str) -> Result<bool>;
    fn put_long_term_control_audit(&self, event: &LongTermMemoryControlAuditEvent) -> Result<()>;
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
        entries.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
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
    revisions: Mutex<Vec<LongTermMemoryControlRevision>>,
    tombstones: Mutex<BTreeMap<String, LongTermMemoryTombstone>>,
    policies: Mutex<BTreeMap<String, Option<MemoryLongTermGovernancePolicy>>>,
    audits: Mutex<Vec<LongTermMemoryControlAuditEvent>>,
    writes: Mutex<Vec<LongTermMemoryControlWrite>>,
}

impl<'a> PlanningLongTermMemoryControlStore<'a> {
    fn new(inner: &'a dyn LongTermMemoryControlReadStore) -> Self {
        Self {
            inner,
            revisions: Mutex::new(Vec::new()),
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
        revisions.extend(
            self.revisions
                .lock()
                .expect("control revisions lock")
                .iter()
                .filter(|revision| revision.record_id == record_id)
                .cloned(),
        );
        revisions.sort_by(|left, right| right.owner_revision.cmp(&left.owner_revision));
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
        tombstones.sort_by(|left, right| right.created_at.cmp(&left.created_at));
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
        policies.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        policies.truncate(limit);
        Ok(policies)
    }

    fn list_long_term_control_audit(
        &self,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryControlAuditEvent>> {
        let mut audits = self.inner.list_long_term_control_audit(usize::MAX)?;
        audits.extend(self.audits.lock().expect("control audits lock").clone());
        audits.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        audits.truncate(limit);
        Ok(audits)
    }
}

impl LongTermMemoryControlStore for PlanningLongTermMemoryControlStore<'_> {
    fn put_long_term_control_revision(
        &self,
        revision: &LongTermMemoryControlRevision,
    ) -> Result<()> {
        self.revisions
            .lock()
            .expect("control revisions lock")
            .push(revision.clone());
        self.writes
            .lock()
            .expect("control writes lock")
            .push(LongTermMemoryControlWrite::PutRevision(revision.clone()));
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
            control_effects_from_writes(&self.writes.lock().expect("control writes lock"));
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
    let resolved = resolve_target(store, control_store, &request.target, false)?;
    let raw_record = resolved.records.first().cloned();
    let record_id = raw_record.as_ref().map(|entry| entry.id.clone());
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
    let previous_digest = digest_entry(&previous);
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
                    subject_visibility: MemorySubjectVisibilityPolicy::AllSubjects,
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
    let new_digest = digest_entry(&updated);
    if !request.dry_run {
        let revision = LongTermMemoryControlRevision {
            schema_version: LONG_TERM_CONTROL_SCHEMA_VERSION,
            revision_id: stable_id(
                "ltmr",
                &(
                    "correct",
                    &previous.id,
                    updated.owner_revision,
                    request.now_secs,
                ),
            ),
            record_id: previous.id.clone(),
            successor_record_id: None,
            operation: "correct".to_string(),
            owner_revision: updated.owner_revision,
            source_revision: updated.source_revision,
            previous_digest: previous_digest.clone(),
            new_digest: new_digest.clone(),
            reason: request.reason.clone(),
            actor_subject_id: request.actor_subject_id.clone(),
            memory_space_id: request.memory_space_id.clone(),
            created_at: request.now_secs,
        };
        control_store.put_long_term_control_revision(&revision)?;
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
            subject_visibility: MemorySubjectVisibilityPolicy::AllSubjects,
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
    let replacement_plan =
        plan_long_term_memory_upsert(store.get(&new_id)?.as_ref(), replacement, request.now_secs);
    let replacement_is_noop = matches!(&replacement_plan, LongTermMemoryEntryPlan::Noop);
    let replacement_entry = match replacement_plan {
        LongTermMemoryEntryPlan::Created(entry) | LongTermMemoryEntryPlan::Updated(entry) => entry,
        LongTermMemoryEntryPlan::Noop => store.get(&new_id)?.ok_or_else(|| {
            Error::config(
                "long_term_control_supersede",
                "replacement planner returned noop without an existing record",
            )
        })?,
        LongTermMemoryEntryPlan::Rejected(reason) => {
            return Ok(rejected_report(
                "supersede",
                request,
                resolved.report,
                format!("replacement_rejected:{reason:?}"),
                None,
            ))
        }
    };
    let previous_digest = digest_entry(&previous);
    let new_digest = digest_entry(&replacement_entry);
    let tombstone = tombstone_for(&previous, "supersede", request);
    if !request.dry_run {
        control_store.put_long_term_control_tombstone(&tombstone)?;
        store.delete(&previous.id)?;
        if !replacement_is_noop {
            store.upsert_many(std::slice::from_ref(replacement), request.now_secs)?;
        }
        control_store.put_long_term_control_revision(&LongTermMemoryControlRevision {
            schema_version: LONG_TERM_CONTROL_SCHEMA_VERSION,
            revision_id: stable_id(
                "ltmr",
                &("supersede", &previous.id, &new_id, request.now_secs),
            ),
            record_id: previous.id.clone(),
            successor_record_id: Some(new_id.clone()),
            operation: "supersede".to_string(),
            owner_revision: replacement_entry.owner_revision,
            source_revision: replacement_entry.source_revision,
            previous_digest: previous_digest.clone(),
            new_digest: new_digest.clone(),
            reason: request.reason.clone(),
            actor_subject_id: request.actor_subject_id.clone(),
            memory_space_id: request.memory_space_id.clone(),
            created_at: request.now_secs,
        })?;
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
            operation: "supersede".to_string(),
            last_owner_revision: previous.owner_revision,
            last_source_revision: previous.source_revision,
        }],
        audit_event_id,
        MemoryProjectionImpactReport {
            affected_record_ids: vec![previous.id.clone(), new_id],
            subject_visibility: MemorySubjectVisibilityPolicy::AllSubjects,
            recall_projection_must_refresh: true,
            notes: Vec::new(),
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
    let previous_digest = digest_entry(&previous);
    let tombstone = tombstone_for(&previous, operation_label, request);
    if !request.dry_run {
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
            operation: operation_label.to_string(),
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
        let previous_digest = digest_entry(previous);
        let tombstone = tombstone_for(previous, "forget_by_query", request);
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
            operation: "forget_by_query".to_string(),
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
        MemorySubjectVisibilityPolicy::AllSubjects,
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
    let notes = if matches!(
        subject_visibility,
        MemorySubjectVisibilityPolicy::AllSubjects
    ) {
        Vec::new()
    } else {
        vec!["report_only_subject_visibility_not_indexed".to_string()]
    };
    apply_owner_field_mutation(
        "change_scope",
        store,
        control_store,
        request,
        resolved,
        previous,
        LongTermMemoryOwnerMutation::ChangeScope(source_scope),
        subject_visibility,
        notes,
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
        MemorySubjectVisibilityPolicy::AllSubjects,
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
    subject_visibility: MemorySubjectVisibilityPolicy,
    notes: Vec<String>,
) -> Result<MemoryLongTermMutationReport> {
    let previous_digest = digest_entry(&previous);
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
                    subject_visibility,
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
    let new_digest = digest_entry(&updated);
    if !request.dry_run {
        control_store.put_long_term_control_revision(&LongTermMemoryControlRevision {
            schema_version: LONG_TERM_CONTROL_SCHEMA_VERSION,
            revision_id: stable_id(
                "ltmr",
                &(
                    operation,
                    &previous.id,
                    updated.owner_revision,
                    request.now_secs,
                ),
            ),
            record_id: previous.id.clone(),
            successor_record_id: None,
            operation: operation.to_string(),
            owner_revision: updated.owner_revision,
            source_revision: updated.source_revision,
            previous_digest: previous_digest.clone(),
            new_digest: new_digest.clone(),
            reason: request.reason.clone(),
            actor_subject_id: request.actor_subject_id.clone(),
            memory_space_id: request.memory_space_id.clone(),
            created_at: request.now_secs,
        })?;
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
            subject_visibility,
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
    operation: &str,
    request: &LongTermMemoryControlMutationRequest,
) -> LongTermMemoryTombstone {
    LongTermMemoryTombstone {
        schema_version: LONG_TERM_CONTROL_SCHEMA_VERSION,
        tombstone_id: stable_id("ltmt", &(operation, &entry.id, request.now_secs)),
        record_id: entry.id.clone(),
        operation: operation.to_string(),
        last_owner_revision: entry.owner_revision,
        last_source_revision: entry.source_revision,
        previous_digest: digest_entry(entry),
        reason: request.reason.clone(),
        actor_subject_id: request.actor_subject_id.clone(),
        memory_space_id: request.memory_space_id.clone(),
        created_at: request.now_secs,
    }
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
    let event_id = stable_id("ltma", &(operation, record_ids, request.now_secs));
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
                .list_long_term_control_revisions(record_id, usize::MAX)?
                .into_iter()
                .filter(|revision| {
                    revision.operation == operation
                        && revision.created_at == request.now_secs
                        && revision.memory_space_id.as_deref() == Some(memory_space_id)
                })
                .map(|revision| ControlEffectRef::Revision {
                    revision_id: revision.revision_id,
                    record_id: revision.record_id,
                    successor_record_id: revision.successor_record_id,
                    owner_revision: revision.owner_revision,
                    source_revision: revision.source_revision,
                }),
        );
        if let Some(tombstone) = control_store.get_long_term_control_tombstone(record_id)? {
            if tombstone.operation == operation
                && tombstone.created_at == request.now_secs
                && tombstone.memory_space_id.as_deref() == Some(memory_space_id)
            {
                effects.push(ControlEffectRef::Tombstone {
                    tombstone_id: tombstone.tombstone_id,
                    record_id: tombstone.record_id,
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
    control_store.put_long_term_control_audit(&LongTermMemoryControlAuditEvent::new(
        event_id.clone(),
        event_id.clone(),
        typed_operation,
        effects,
        request.reason.clone(),
        request.actor_subject_id.clone(),
        memory_space_id,
        request.now_secs,
    ))?;
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
    let policy_ids = policies
        .iter()
        .map(|policy| policy.policy_id.as_str())
        .collect::<Vec<_>>();
    let event_id = stable_id("ltma", &(operation, policy_ids, now_secs));
    let effects = policies
        .iter()
        .map(|policy| ControlEffectRef::Policy {
            policy_id: policy.policy_id.clone(),
            policy_revision: policy.policy_revision,
            deleted,
        })
        .collect();
    control_store.put_long_term_control_audit(&LongTermMemoryControlAuditEvent::new(
        event_id.clone(),
        event_id.clone(),
        typed_operation,
        effects,
        reason,
        None,
        memory_space_id,
        now_secs,
    ))?;
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
        MemoryLongTermMutation::Delete { .. } => "delete",
        MemoryLongTermMutation::ForgetByQuery { .. } => "forget_by_query",
        MemoryLongTermMutation::MarkStale { .. } => "mark_stale",
        MemoryLongTermMutation::ChangeScope { .. } => "change_scope",
        MemoryLongTermMutation::ChangePrivacy { .. } => "change_privacy",
    }
}

fn digest_entry(entry: &LongTermMemoryEntry) -> String {
    let mut hasher = DefaultHasher::new();
    "canonical_long_term_owner_v2".hash(&mut hasher);
    entry.id.hash(&mut hasher);
    entry.kind.hash(&mut hasher);
    entry.topic.hash(&mut hasher);
    entry.content.hash(&mut hasher);
    entry.keywords.hash(&mut hasher);
    entry.privacy.label().hash(&mut hasher);
    entry.source_chat_id.hash(&mut hasher);
    entry.source_type.hash(&mut hasher);
    entry.source_scope.hash(&mut hasher);
    entry.confidence.hash(&mut hasher);
    entry.freshness.hash(&mut hasher);
    entry.stale_hint.hash(&mut hasher);
    entry.supporting_citations.hash(&mut hasher);
    entry.canonical_entities.hash(&mut hasher);
    entry.evidence_count.hash(&mut hasher);
    entry.observed_at.hash(&mut hasher);
    entry.last_confirmed_at.hash(&mut hasher);
    entry.source_revision.hash(&mut hasher);
    entry.owner_revision.hash(&mut hasher);
    format!("ltmd-{:016x}", hasher.finish())
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

        assert_ne!(digest_entry(&baseline), digest_entry(&changed));
    }

    #[test]
    fn canonical_owner_digest_covers_governed_owner_contract() {
        let baseline = owner_fixture();
        let baseline_digest = digest_entry(&baseline);
        let mut variants = Vec::new();

        let mut changed = baseline.clone();
        changed.content.push_str(" Updated.");
        variants.push(("content", changed));

        let mut changed = baseline.clone();
        changed.source_scope = LongTermMemorySourceScope::World;
        variants.push(("source_scope", changed));

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
                digest_entry(&changed),
                "governed owner field missing from digest: {field}"
            );
        }
    }
}
