use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::error::{Error, Result};

use super::{
    long_term_memory_evidence_summary, DerivedMemoryPlane, DerivedMemoryRef, LongTermMemoryDraft,
    LongTermMemoryEntry, LongTermMemoryKind, LongTermMemoryQuery, LongTermMemorySlot,
    LongTermMemorySourceScope, LongTermMemoryStaleHint, LongTermMemoryStore, SubjectId,
    TranscriptEvidenceRef,
};

pub const LONG_TERM_CONTROL_REVISION_NAMESPACE: &str = "long_term_control_revision";
pub const LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE: &str = "long_term_control_tombstone";
pub const LONG_TERM_GOVERNANCE_POLICY_NAMESPACE: &str = "long_term_governance_policy";
pub const LONG_TERM_CONTROL_AUDIT_NAMESPACE: &str = "long_term_control_audit";

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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemorySubjectVisibilityPolicy {
    AllSubjects,
    OnlySubjects(Vec<SubjectId>),
    HiddenFromSubjects(Vec<SubjectId>),
}

impl Default for MemorySubjectVisibilityPolicy {
    fn default() -> Self {
        Self::AllSubjects
    }
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
    pub previous_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_revision: Option<u64>,
    pub previous_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_digest: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryLongTermTombstoneRef {
    pub record_id: String,
    pub tombstone_id: String,
    pub operation: String,
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
}

impl Default for MemoryProjectionImpactReport {
    fn default() -> Self {
        Self {
            affected_record_ids: Vec::new(),
            subject_visibility: MemorySubjectVisibilityPolicy::AllSubjects,
            recall_projection_must_refresh: false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryDeferredGovernanceImpactReport {
    pub policy_ids: Vec<String>,
    pub deferred_jobs_may_be_affected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryLongTermMutationReport {
    pub accepted: bool,
    pub dry_run: bool,
    pub operation: &'static str,
    pub target_report: MemoryLongTermTargetResolutionReport,
    pub affected_records: Vec<MemoryLongTermAffectedRecord>,
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
    pub revision_id: String,
    pub record_id: String,
    pub operation: String,
    pub revision: u64,
    pub previous_digest: String,
    pub new_digest: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_subject_id: Option<SubjectId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_space_id: Option<String>,
    pub created_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LongTermMemoryTombstone {
    pub tombstone_id: String,
    pub record_id: String,
    pub operation: String,
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
    pub event_id: String,
    pub operation: String,
    pub record_ids: Vec<String>,
    pub policy_ids: Vec<String>,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_subject_id: Option<SubjectId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_space_id: Option<String>,
    pub created_at: u64,
}

pub trait LongTermMemoryControlStore: Send + Sync {
    fn put_long_term_control_revision(
        &self,
        revision: &LongTermMemoryControlRevision,
    ) -> Result<()>;
    fn list_long_term_control_revisions(
        &self,
        record_id: &str,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryControlRevision>>;
    fn put_long_term_control_tombstone(&self, tombstone: &LongTermMemoryTombstone) -> Result<()>;
    fn get_long_term_control_tombstone(
        &self,
        record_id: &str,
    ) -> Result<Option<LongTermMemoryTombstone>>;
    fn list_long_term_control_tombstones(
        &self,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryTombstone>>;
    fn put_long_term_governance_policy(
        &self,
        policy: &MemoryLongTermGovernancePolicy,
    ) -> Result<()>;
    fn delete_long_term_governance_policy(&self, policy_id: &str) -> Result<bool>;
    fn list_long_term_governance_policies(
        &self,
        limit: usize,
    ) -> Result<Vec<MemoryLongTermGovernancePolicy>>;
    fn put_long_term_control_audit(&self, event: &LongTermMemoryControlAuditEvent) -> Result<()>;
    fn list_long_term_control_audit(
        &self,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryControlAuditEvent>>;
}

pub fn list_long_term_memory_control_page(
    store: &dyn LongTermMemoryStore,
    control_store: &dyn LongTermMemoryControlStore,
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
    store: &dyn LongTermMemoryStore,
    control_store: &dyn LongTermMemoryControlStore,
    request: LongTermMemoryControlDetailRequest,
) -> Result<MemoryLongTermDetailReport> {
    let resolved = resolve_target(store, control_store, &request.target, false)?;
    let raw_record = resolved.records.first().cloned();
    let record_id = raw_record
        .as_ref()
        .map(|entry| entry.id.clone())
        .or_else(|| resolved.record_ids.first().cloned())
        .or_else(|| target_record_id_hint(&request.target));
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

pub fn apply_long_term_memory_control_mutation(
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
    }
}

pub fn apply_long_term_memory_governance_policy_mutation(
    control_store: &dyn LongTermMemoryControlStore,
    operation: MemoryGovernancePolicyMutation,
    reason: String,
    dry_run: bool,
    now_secs: u64,
) -> Result<MemoryGovernancePolicyMutationReport> {
    match operation {
        MemoryGovernancePolicyMutation::Suppress { selector, duration } => {
            let policy_id = stable_id("ltmp", &("suppress", &selector, &duration));
            let policy = MemoryLongTermGovernancePolicy {
                policy_id: policy_id.clone(),
                kind: "suppress".to_string(),
                selector,
                duration: Some(duration),
                expires_at: None,
                reason: reason.clone(),
                created_at: now_secs,
                updated_at: now_secs,
            };
            if !dry_run {
                control_store.put_long_term_governance_policy(&policy)?;
            }
            let audit_event_id = write_policy_audit(
                control_store,
                "policy.suppress",
                &policy_id,
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
            let policy_id = stable_id("ltmp", &("pause", &selector, expires_at));
            let policy = MemoryLongTermGovernancePolicy {
                policy_id: policy_id.clone(),
                kind: "pause".to_string(),
                selector,
                duration: None,
                expires_at,
                reason: reason.clone(),
                created_at: now_secs,
                updated_at: now_secs,
            };
            if !dry_run {
                control_store.put_long_term_governance_policy(&policy)?;
            }
            let audit_event_id = write_policy_audit(
                control_store,
                "policy.pause",
                &policy_id,
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
    let audit_event_id = policy_id
        .as_deref()
        .map(|id| write_policy_audit(control_store, operation, id, &reason, now_secs, dry_run))
        .transpose()?
        .flatten();
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
    let new_revision = previous.source_revision.saturating_add(1).max(1);
    let mut replacement = replacement.clone();
    replacement.source_revision = Some(new_revision);
    let new_digest = digest_draft(&replacement);
    let previous_digest = digest_entry(&previous);
    if !request.dry_run {
        store.upsert_many(&[replacement], request.now_secs)?;
        let revision = LongTermMemoryControlRevision {
            revision_id: stable_id(
                "ltmr",
                &("correct", &previous.id, new_revision, request.now_secs),
            ),
            record_id: previous.id.clone(),
            operation: "correct".to_string(),
            revision: new_revision,
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
        &[previous.id.clone()],
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
            previous_revision: previous.source_revision,
            new_revision: Some(new_revision),
            previous_digest,
            new_digest: Some(new_digest),
        }],
        Vec::new(),
        audit_event_id,
        MemoryProjectionImpactReport {
            affected_record_ids: vec![previous.id.clone()],
            subject_visibility: MemorySubjectVisibilityPolicy::AllSubjects,
            recall_projection_must_refresh: true,
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
    let previous_digest = digest_entry(&previous);
    let new_digest = digest_draft(replacement);
    let tombstone = tombstone_for(&previous, "supersede", request);
    if !request.dry_run {
        control_store.put_long_term_control_tombstone(&tombstone)?;
        store.delete(&previous.id)?;
        store.upsert_many(&[replacement.clone()], request.now_secs)?;
        control_store.put_long_term_control_revision(&LongTermMemoryControlRevision {
            revision_id: stable_id(
                "ltmr",
                &("supersede", &previous.id, &new_id, request.now_secs),
            ),
            record_id: previous.id.clone(),
            operation: "supersede".to_string(),
            revision: previous.source_revision.saturating_add(1).max(1),
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
            previous_revision: previous.source_revision,
            new_revision: Some(previous.source_revision.saturating_add(1).max(1)),
            previous_digest,
            new_digest: Some(new_digest),
        }],
        vec![MemoryLongTermTombstoneRef {
            record_id: previous.id.clone(),
            tombstone_id: tombstone.tombstone_id,
            operation: "supersede".to_string(),
        }],
        audit_event_id,
        MemoryProjectionImpactReport {
            affected_record_ids: vec![previous.id.clone(), new_id],
            subject_visibility: MemorySubjectVisibilityPolicy::AllSubjects,
            recall_projection_must_refresh: true,
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
        &[previous.id.clone()],
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
            previous_revision: previous.source_revision,
            new_revision: None,
            previous_digest,
            new_digest: None,
        }],
        vec![MemoryLongTermTombstoneRef {
            record_id: previous.id.clone(),
            tombstone_id: tombstone.tombstone_id,
            operation: operation_label.to_string(),
        }],
        audit_event_id,
        MemoryProjectionImpactReport {
            affected_record_ids: vec![previous.id.clone()],
            subject_visibility: MemorySubjectVisibilityPolicy::AllSubjects,
            recall_projection_must_refresh: true,
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
            previous_revision: previous.source_revision,
            new_revision: None,
            previous_digest,
            new_digest: None,
        });
        tombstones.push(MemoryLongTermTombstoneRef {
            record_id: previous.id.clone(),
            tombstone_id: tombstone.tombstone_id,
            operation: "forget_by_query".to_string(),
        });
    }
    let audit_event_id = write_record_audit(
        control_store,
        "forget_by_query",
        &resolved.record_ids,
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
    let mut replacement = LongTermMemoryDraft {
        kind: previous.kind.clone(),
        topic: previous.topic.clone(),
        content: previous.content.clone(),
        keywords: previous.keywords.clone(),
        source_chat_id: previous.source_chat_id.clone(),
        source_type: Some(previous.source_type),
        source_scope: Some(previous.source_scope),
        confidence: Some(previous.confidence),
        freshness: Some(previous.freshness),
        stale_hint: Some(stale_hint),
        supporting_citations: previous.supporting_citations.clone(),
        evidence_count: Some(previous.evidence_count),
        observed_at: Some(previous.observed_at),
        last_confirmed_at: Some(previous.last_confirmed_at),
        source_revision: Some(previous.source_revision.saturating_add(1).max(1)),
    };
    replacement.stale_hint = Some(stale_hint);
    apply_correct(
        store,
        control_store,
        request,
        &MemoryLongTermTarget::RecordId(previous.id.clone()),
        &replacement,
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
    let replacement = LongTermMemoryDraft {
        kind: previous.kind.clone(),
        topic: previous.topic.clone(),
        content: previous.content.clone(),
        keywords: previous.keywords.clone(),
        source_chat_id: previous.source_chat_id.clone(),
        source_type: Some(previous.source_type),
        source_scope: Some(source_scope),
        confidence: Some(previous.confidence),
        freshness: Some(previous.freshness),
        stale_hint: Some(previous.stale_hint),
        supporting_citations: previous.supporting_citations.clone(),
        evidence_count: Some(previous.evidence_count),
        observed_at: Some(previous.observed_at),
        last_confirmed_at: Some(previous.last_confirmed_at),
        source_revision: Some(previous.source_revision.saturating_add(1).max(1)),
    };
    let previous_digest = digest_entry(&previous);
    let new_digest = digest_draft(&replacement);
    if !request.dry_run {
        store.upsert_many(&[replacement], request.now_secs)?;
        control_store.put_long_term_control_revision(&LongTermMemoryControlRevision {
            revision_id: stable_id("ltmr", &("change_scope", &previous.id, request.now_secs)),
            record_id: previous.id.clone(),
            operation: "change_scope".to_string(),
            revision: previous.source_revision.saturating_add(1).max(1),
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
        "change_scope",
        &[previous.id.clone()],
        request,
        request.dry_run,
    )?;
    Ok(accepted_record_report(
        "change_scope",
        request,
        resolved,
        vec![MemoryLongTermAffectedRecord {
            record_id: previous.id.clone(),
            operation: "change_scope".to_string(),
            previous_revision: previous.source_revision,
            new_revision: Some(previous.source_revision.saturating_add(1).max(1)),
            previous_digest,
            new_digest: Some(new_digest),
        }],
        Vec::new(),
        audit_event_id,
        MemoryProjectionImpactReport {
            affected_record_ids: vec![previous.id.clone()],
            subject_visibility,
            recall_projection_must_refresh: true,
        },
    ))
}

struct ResolvedTarget {
    report: MemoryLongTermTargetResolutionReport,
    records: Vec<LongTermMemoryEntry>,
    record_ids: Vec<String>,
}

fn resolve_target(
    store: &dyn LongTermMemoryStore,
    control_store: &dyn LongTermMemoryControlStore,
    target: &MemoryLongTermTarget,
    allow_query: bool,
) -> Result<ResolvedTarget> {
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

fn target_record_id_hint(target: &MemoryLongTermTarget) -> Option<String> {
    match target {
        MemoryLongTermTarget::RecordId(id) => {
            Some(id.trim().to_string()).filter(|id| !id.is_empty())
        }
        MemoryLongTermTarget::Slot(slot) => slot.stable_id(),
        MemoryLongTermTarget::TranscriptDerivedRef(reference) => {
            Some(long_term_record_id_from_derived_ref(reference))
        }
        MemoryLongTermTarget::Query(_) => None,
    }
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
    control_store: &dyn LongTermMemoryControlStore,
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

fn is_tombstoned(control_store: &dyn LongTermMemoryControlStore, record_id: &str) -> bool {
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
        tombstone_id: stable_id("ltmt", &(operation, &entry.id, request.now_secs)),
        record_id: entry.id.clone(),
        operation: operation.to_string(),
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
    request: &LongTermMemoryControlMutationRequest,
    dry_run: bool,
) -> Result<Option<String>> {
    if dry_run {
        return Ok(None);
    }
    let event_id = stable_id("ltma", &(operation, record_ids, request.now_secs));
    control_store.put_long_term_control_audit(&LongTermMemoryControlAuditEvent {
        event_id: event_id.clone(),
        operation: operation.to_string(),
        record_ids: record_ids.to_vec(),
        policy_ids: Vec::new(),
        reason: request.reason.clone(),
        actor_subject_id: request.actor_subject_id.clone(),
        memory_space_id: request.memory_space_id.clone(),
        created_at: request.now_secs,
    })?;
    Ok(Some(event_id))
}

fn write_policy_audit(
    control_store: &dyn LongTermMemoryControlStore,
    operation: &str,
    policy_id: &str,
    reason: &str,
    now_secs: u64,
    dry_run: bool,
) -> Result<Option<String>> {
    if dry_run {
        return Ok(None);
    }
    let event_id = stable_id("ltma", &(operation, policy_id, now_secs));
    control_store.put_long_term_control_audit(&LongTermMemoryControlAuditEvent {
        event_id: event_id.clone(),
        operation: operation.to_string(),
        record_ids: Vec::new(),
        policy_ids: vec![policy_id.to_string()],
        reason: reason.to_string(),
        actor_subject_id: None,
        memory_space_id: None,
        created_at: now_secs,
    })?;
    Ok(Some(event_id))
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
    }
}

fn digest_entry(entry: &LongTermMemoryEntry) -> String {
    stable_id(
        "ltmd",
        &(
            &entry.id,
            &entry.kind,
            &entry.topic,
            &entry.content,
            &entry.keywords,
            entry.source_revision,
        ),
    )
}

fn digest_draft(draft: &LongTermMemoryDraft) -> String {
    stable_id(
        "ltmd",
        &(
            &draft.kind,
            &draft.topic,
            &draft.content,
            &draft.keywords,
            draft.source_revision,
        ),
    )
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
