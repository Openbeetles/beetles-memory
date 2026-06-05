use std::collections::BTreeMap;
use std::sync::Mutex;

use bm_core::memory::{
    apply_long_term_memory_control_mutation, apply_long_term_memory_governance_policy_mutation,
    get_long_term_memory_control_detail, list_long_term_memory_control_page, DerivedMemoryPlane,
    DerivedMemoryRef, LongTermMemoryControlAuditEvent, LongTermMemoryControlDetailRequest,
    LongTermMemoryControlListRequest, LongTermMemoryControlMutationRequest,
    LongTermMemoryControlStore, LongTermMemoryDraft, LongTermMemoryEntry, LongTermMemoryKind,
    LongTermMemoryQuery, LongTermMemorySlot, LongTermMemorySourceScope, LongTermMemoryStore,
    MemoryGovernancePolicyMutation, MemoryGovernanceSelector, MemoryGovernanceSuppressionDuration,
    MemoryLongTermControlView, MemoryLongTermGovernancePolicy, MemoryLongTermMutation,
    MemoryLongTermSelector, MemoryLongTermTarget, MemorySubjectVisibilityPolicy,
    TranscriptEvidenceRef,
};
use bm_core::Result;

const NOW_SECS: u64 = 1_780_000_000;

#[derive(Default)]
struct InMemoryLongTermStore {
    entries: Mutex<BTreeMap<String, LongTermMemoryEntry>>,
}

impl InMemoryLongTermStore {
    fn seed(&self, draft: LongTermMemoryDraft) -> String {
        let id = draft.stable_id().expect("stable id");
        self.upsert_many(&[draft], NOW_SECS)
            .expect("seed long term");
        id
    }
}

impl LongTermMemoryStore for InMemoryLongTermStore {
    fn upsert_many(&self, drafts: &[LongTermMemoryDraft], now_secs: u64) -> Result<usize> {
        let mut entries = self.entries.lock().expect("entries lock");
        let mut changed = 0usize;
        for draft in drafts {
            let Some(normalized) = draft.normalized() else {
                continue;
            };
            let Some(id) = normalized.stable_id() else {
                continue;
            };
            let prior_created_at = entries.get(&id).map(|entry| entry.created_at);
            let prior_last_used_at = entries.get(&id).map(|entry| entry.last_used_at);
            entries.insert(
                id.clone(),
                LongTermMemoryEntry {
                    id,
                    kind: normalized.kind,
                    topic: normalized.topic,
                    content: normalized.content,
                    keywords: normalized.keywords,
                    source_chat_id: normalized.source_chat_id,
                    source_type: normalized.source_type.unwrap_or_default(),
                    source_scope: normalized.source_scope.unwrap_or_default(),
                    confidence: normalized.confidence.unwrap_or_default(),
                    freshness: normalized.freshness.unwrap_or_default(),
                    stale_hint: normalized.stale_hint.unwrap_or_default(),
                    supporting_citations: normalized.supporting_citations,
                    evidence_count: normalized.evidence_count.unwrap_or(0),
                    created_at: prior_created_at.unwrap_or(now_secs),
                    updated_at: now_secs,
                    observed_at: normalized.observed_at.unwrap_or(now_secs),
                    last_confirmed_at: normalized.last_confirmed_at.unwrap_or(now_secs),
                    source_revision: normalized.source_revision.unwrap_or(0),
                    last_used_at: prior_last_used_at.unwrap_or(0),
                },
            );
            changed += 1;
        }
        Ok(changed)
    }

    fn recall(
        &self,
        query: &str,
        source_chat_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryEntry>> {
        let query = query.trim().to_lowercase();
        let mut entries = self
            .entries
            .lock()
            .expect("entries lock")
            .values()
            .filter(|entry| {
                (query.is_empty()
                    || entry.topic.to_lowercase().contains(&query)
                    || entry.content.to_lowercase().contains(&query))
                    && source_chat_id
                        .map(|chat_id| entry.source_chat_id.as_deref() == Some(chat_id))
                        .unwrap_or(true)
            })
            .cloned()
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        entries.truncate(limit);
        Ok(entries)
    }

    fn get(&self, id: &str) -> Result<Option<LongTermMemoryEntry>> {
        Ok(self.entries.lock().expect("entries lock").get(id).cloned())
    }

    fn list(&self, limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
        let mut entries = self
            .entries
            .lock()
            .expect("entries lock")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        entries.truncate(limit);
        Ok(entries)
    }

    fn delete(&self, id: &str) -> Result<bool> {
        Ok(self
            .entries
            .lock()
            .expect("entries lock")
            .remove(id)
            .is_some())
    }

    fn delete_slot(&self, slot: &LongTermMemorySlot) -> Result<bool> {
        let Some(id) = slot.stable_id() else {
            return Ok(false);
        };
        self.delete(&id)
    }

    fn count(&self) -> Result<usize> {
        Ok(self.entries.lock().expect("entries lock").len())
    }
}

#[derive(Default)]
struct InMemoryControlStore {
    revisions: Mutex<Vec<bm_core::memory::LongTermMemoryControlRevision>>,
    tombstones: Mutex<BTreeMap<String, bm_core::memory::LongTermMemoryTombstone>>,
    policies: Mutex<BTreeMap<String, MemoryLongTermGovernancePolicy>>,
    audits: Mutex<Vec<LongTermMemoryControlAuditEvent>>,
}

impl LongTermMemoryControlStore for InMemoryControlStore {
    fn put_long_term_control_revision(
        &self,
        revision: &bm_core::memory::LongTermMemoryControlRevision,
    ) -> Result<()> {
        self.revisions
            .lock()
            .expect("revisions lock")
            .push(revision.clone());
        Ok(())
    }

    fn list_long_term_control_revisions(
        &self,
        record_id: &str,
        limit: usize,
    ) -> Result<Vec<bm_core::memory::LongTermMemoryControlRevision>> {
        let mut revisions = self
            .revisions
            .lock()
            .expect("revisions lock")
            .iter()
            .filter(|revision| revision.record_id == record_id)
            .cloned()
            .collect::<Vec<_>>();
        revisions.sort_by(|left, right| right.revision.cmp(&left.revision));
        revisions.truncate(limit);
        Ok(revisions)
    }

    fn put_long_term_control_tombstone(
        &self,
        tombstone: &bm_core::memory::LongTermMemoryTombstone,
    ) -> Result<()> {
        self.tombstones
            .lock()
            .expect("tombstones lock")
            .insert(tombstone.record_id.clone(), tombstone.clone());
        Ok(())
    }

    fn get_long_term_control_tombstone(
        &self,
        record_id: &str,
    ) -> Result<Option<bm_core::memory::LongTermMemoryTombstone>> {
        Ok(self
            .tombstones
            .lock()
            .expect("tombstones lock")
            .get(record_id)
            .cloned())
    }

    fn list_long_term_control_tombstones(
        &self,
        limit: usize,
    ) -> Result<Vec<bm_core::memory::LongTermMemoryTombstone>> {
        let mut tombstones = self
            .tombstones
            .lock()
            .expect("tombstones lock")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        tombstones.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        tombstones.truncate(limit);
        Ok(tombstones)
    }

    fn put_long_term_governance_policy(
        &self,
        policy: &MemoryLongTermGovernancePolicy,
    ) -> Result<()> {
        self.policies
            .lock()
            .expect("policies lock")
            .insert(policy.policy_id.clone(), policy.clone());
        Ok(())
    }

    fn delete_long_term_governance_policy(&self, policy_id: &str) -> Result<bool> {
        Ok(self
            .policies
            .lock()
            .expect("policies lock")
            .remove(policy_id)
            .is_some())
    }

    fn list_long_term_governance_policies(
        &self,
        limit: usize,
    ) -> Result<Vec<MemoryLongTermGovernancePolicy>> {
        let mut policies = self
            .policies
            .lock()
            .expect("policies lock")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        policies.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        policies.truncate(limit);
        Ok(policies)
    }

    fn put_long_term_control_audit(&self, event: &LongTermMemoryControlAuditEvent) -> Result<()> {
        self.audits.lock().expect("audits lock").push(event.clone());
        Ok(())
    }

    fn list_long_term_control_audit(
        &self,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryControlAuditEvent>> {
        let mut audits = self.audits.lock().expect("audits lock").clone();
        audits.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        audits.truncate(limit);
        Ok(audits)
    }
}

fn draft(kind: LongTermMemoryKind, topic: &str, content: &str) -> LongTermMemoryDraft {
    LongTermMemoryDraft {
        kind,
        topic: topic.to_string(),
        content: content.to_string(),
        keywords: vec![topic.to_string()],
        source_chat_id: Some("conversation-a".to_string()),
        source_type: None,
        source_scope: Some(LongTermMemorySourceScope::User),
        confidence: None,
        freshness: None,
        stale_hint: None,
        supporting_citations: vec![transcript_ref().display_citation()],
        evidence_count: Some(1),
        observed_at: Some(NOW_SECS - 10),
        last_confirmed_at: Some(NOW_SECS - 10),
        source_revision: Some(1),
    }
}

fn transcript_ref() -> TranscriptEvidenceRef {
    transcript_ref_for("turn-1", Some("message-1"))
}

fn transcript_ref_for(turn_id: &str, message_id: Option<&str>) -> TranscriptEvidenceRef {
    TranscriptEvidenceRef {
        memory_space_id: "space-user".to_string(),
        channel_id: "chat".to_string(),
        conversation_id: "conversation-a".to_string(),
        turn_id: turn_id.to_string(),
        message_id: message_id.map(str::to_string),
        subject_id: None,
        authority: None,
    }
}

fn draft_with_ref(
    kind: LongTermMemoryKind,
    topic: &str,
    content: &str,
    evidence_ref: TranscriptEvidenceRef,
) -> LongTermMemoryDraft {
    let mut draft = draft(kind, topic, content);
    draft.supporting_citations = vec![evidence_ref.display_citation()];
    draft
}

fn derived_ref(record_id: &str) -> DerivedMemoryRef {
    DerivedMemoryRef {
        plane: DerivedMemoryPlane::LongTerm,
        store_key: record_id.to_string(),
        subject_id: Some("subject-human".to_string()),
        source: transcript_ref(),
        created_at: NOW_SECS,
    }
}

#[test]
fn list_and_detail_return_active_records_with_evidence_and_cursor() {
    let store = InMemoryLongTermStore::default();
    let control = InMemoryControlStore::default();
    let first_id = store.seed(draft(
        LongTermMemoryKind::Preference,
        "preferred-editor",
        "The user prefers Helix for quick terminal edits.",
    ));
    store.seed(draft(
        LongTermMemoryKind::Project,
        "release-gate",
        "The user expects release gates before handoff.",
    ));

    let page = list_long_term_memory_control_page(
        &store,
        &control,
        LongTermMemoryControlListRequest {
            query: LongTermMemoryQuery {
                kind: Some(LongTermMemoryKind::Preference),
                limit: 10,
                ..LongTermMemoryQuery::default()
            },
            cursor: None,
            limit: 1,
            view: MemoryLongTermControlView::HostUi,
        },
    )
    .expect("list report");

    assert_eq!(page.records.len(), 1);
    assert_eq!(page.records[0].record.id, first_id);
    assert!(page.records[0].record.source_chat_id.is_none());
    assert!(page.records[0].record.supporting_citations.is_empty());
    assert!(!page.records[0].evidence_summary.contains("transcript:"));
    assert_eq!(page.records[0].transcript_refs, vec![transcript_ref()]);
    assert!(page.next_cursor.is_none());

    let detail = get_long_term_memory_control_detail(
        &store,
        &control,
        LongTermMemoryControlDetailRequest {
            target: MemoryLongTermTarget::RecordId(first_id.clone()),
            view: MemoryLongTermControlView::HostUi,
        },
    )
    .expect("detail report");
    let detail_record = detail.record.expect("detail record");
    assert!(detail_record.content.contains("Helix"));
    assert!(detail_record.source_chat_id.is_none());
    assert!(detail_record.supporting_citations.is_empty());
    assert_eq!(detail.transcript_refs, vec![transcript_ref()]);

    let raw_detail = get_long_term_memory_control_detail(
        &store,
        &control,
        LongTermMemoryControlDetailRequest {
            target: MemoryLongTermTarget::RecordId(first_id),
            view: MemoryLongTermControlView::RawOwner,
        },
    )
    .expect("raw owner detail");
    let raw_record = raw_detail.record.expect("raw detail record");
    assert_eq!(raw_record.source_chat_id.as_deref(), Some("conversation-a"));
    assert_eq!(
        raw_record.supporting_citations,
        vec![transcript_ref().display_citation()]
    );
}

#[test]
fn correct_preserves_record_lineage_and_increments_source_revision() {
    let store = InMemoryLongTermStore::default();
    let control = InMemoryControlStore::default();
    let record_id = store.seed(draft(
        LongTermMemoryKind::Preference,
        "preferred-editor",
        "The user prefers Helix for quick terminal edits.",
    ));

    let report = apply_long_term_memory_control_mutation(
        &store,
        &control,
        LongTermMemoryControlMutationRequest {
            operation: MemoryLongTermMutation::Correct {
                target: MemoryLongTermTarget::RecordId(record_id.clone()),
                replacement: draft(
                    LongTermMemoryKind::Preference,
                    "preferred-editor",
                    "The user prefers Neovim for quick terminal edits.",
                ),
            },
            reason: "user_corrected_preference".to_string(),
            dry_run: false,
            actor_subject_id: Some("subject-human".to_string()),
            memory_space_id: Some("space-user".to_string()),
            now_secs: NOW_SECS + 1,
        },
    )
    .expect("correct report");

    assert!(report.accepted);
    assert_eq!(report.affected_records.len(), 1);
    assert_eq!(report.affected_records[0].record_id, record_id);
    assert_eq!(report.affected_records[0].previous_revision, 1);
    assert_eq!(report.affected_records[0].new_revision, Some(2));
    let corrected = store.get(&record_id).expect("read").expect("corrected");
    assert_eq!(corrected.source_revision, 2);
    assert!(corrected.content.contains("Neovim"));
    assert_eq!(
        control
            .list_long_term_control_revisions(&record_id, 10)
            .unwrap()
            .len(),
        1
    );
    assert!(report.audit_event_id.is_some());
}

#[test]
fn supersede_and_delete_create_tombstones_and_exclude_old_records() {
    let store = InMemoryLongTermStore::default();
    let control = InMemoryControlStore::default();
    let old_id = store.seed(draft(
        LongTermMemoryKind::Project,
        "release-gate",
        "The user accepts manual smoke tests as release gate.",
    ));

    let supersede = apply_long_term_memory_control_mutation(
        &store,
        &control,
        LongTermMemoryControlMutationRequest {
            operation: MemoryLongTermMutation::Supersede {
                target: MemoryLongTermTarget::RecordId(old_id.clone()),
                replacement: draft(
                    LongTermMemoryKind::Project,
                    "release-gate-v2",
                    "The user expects automated gates plus manual smoke before release.",
                ),
            },
            reason: "project_process_changed".to_string(),
            dry_run: false,
            actor_subject_id: Some("subject-human".to_string()),
            memory_space_id: Some("space-user".to_string()),
            now_secs: NOW_SECS + 2,
        },
    )
    .expect("supersede");

    assert!(supersede.accepted);
    assert!(store.get(&old_id).unwrap().is_none());
    assert_eq!(supersede.tombstones[0].record_id, old_id);
    assert!(control
        .get_long_term_control_tombstone(&old_id)
        .unwrap()
        .is_some());

    let new_id = draft(
        LongTermMemoryKind::Project,
        "release-gate-v2",
        "The user expects automated gates plus manual smoke before release.",
    )
    .stable_id()
    .unwrap();
    let delete = apply_long_term_memory_control_mutation(
        &store,
        &control,
        LongTermMemoryControlMutationRequest {
            operation: MemoryLongTermMutation::Delete {
                target: MemoryLongTermTarget::RecordId(new_id.clone()),
            },
            reason: "user_deleted_project_memory".to_string(),
            dry_run: false,
            actor_subject_id: Some("subject-human".to_string()),
            memory_space_id: Some("space-user".to_string()),
            now_secs: NOW_SECS + 3,
        },
    )
    .expect("delete");

    assert!(delete.accepted);
    assert!(store.get(&new_id).unwrap().is_none());
    assert!(store.recall("release", None, 10).unwrap().is_empty());
    assert_eq!(
        control.list_long_term_control_tombstones(10).unwrap().len(),
        2
    );

    let deleted_detail = get_long_term_memory_control_detail(
        &store,
        &control,
        LongTermMemoryControlDetailRequest {
            target: MemoryLongTermTarget::RecordId(new_id),
            view: MemoryLongTermControlView::HostUi,
        },
    )
    .expect("deleted detail");
    assert!(deleted_detail.record.is_none());
    assert!(deleted_detail.tombstone.is_some());
}

#[test]
fn forget_by_query_requires_preview_confirmation_before_destructive_change() {
    let store = InMemoryLongTermStore::default();
    let control = InMemoryControlStore::default();
    store.seed(draft(
        LongTermMemoryKind::Relationship,
        "person-alex",
        "The user collaborates with Alex on launch planning.",
    ));

    let rejected = apply_long_term_memory_control_mutation(
        &store,
        &control,
        LongTermMemoryControlMutationRequest {
            operation: MemoryLongTermMutation::ForgetByQuery {
                selector: MemoryLongTermSelector {
                    query: LongTermMemoryQuery {
                        topic: Some("person-alex".to_string()),
                        limit: 10,
                        ..LongTermMemoryQuery::default()
                    },
                    evidence_ref: None,
                },
                confirmation_token: None,
            },
            reason: "user_requested_forget_person".to_string(),
            dry_run: false,
            actor_subject_id: Some("subject-human".to_string()),
            memory_space_id: Some("space-user".to_string()),
            now_secs: NOW_SECS + 4,
        },
    )
    .expect("forget rejected report");

    assert!(!rejected.accepted);
    assert!(rejected
        .policy_decision
        .reason
        .contains("confirmation_required"));
    assert_eq!(store.count().unwrap(), 1);

    let preview = apply_long_term_memory_control_mutation(
        &store,
        &control,
        LongTermMemoryControlMutationRequest {
            operation: MemoryLongTermMutation::ForgetByQuery {
                selector: MemoryLongTermSelector {
                    query: LongTermMemoryQuery {
                        topic: Some("person-alex".to_string()),
                        limit: 10,
                        ..LongTermMemoryQuery::default()
                    },
                    evidence_ref: None,
                },
                confirmation_token: None,
            },
            reason: "user_requested_forget_person".to_string(),
            dry_run: true,
            actor_subject_id: Some("subject-human".to_string()),
            memory_space_id: Some("space-user".to_string()),
            now_secs: NOW_SECS + 5,
        },
    )
    .expect("forget preview");

    assert!(preview.dry_run);
    assert!(!preview.accepted);
    assert_eq!(preview.target_report.resolved_count, 1);
    assert!(preview.policy_decision.confirmation_token.is_some());
    assert_eq!(store.count().unwrap(), 1);
}

#[test]
fn forget_by_query_evidence_ref_narrows_bulk_target() {
    let store = InMemoryLongTermStore::default();
    let control = InMemoryControlStore::default();
    let first_ref = transcript_ref_for("turn-a", Some("message-a"));
    let second_ref = transcript_ref_for("turn-b", Some("message-b"));
    let first_id = store.seed(draft_with_ref(
        LongTermMemoryKind::Preference,
        "temporary-tone",
        "The user temporarily wants terse answers.",
        first_ref,
    ));
    let second_id = store.seed(draft_with_ref(
        LongTermMemoryKind::Preference,
        "temporary-format",
        "The user temporarily wants bullet answers.",
        second_ref.clone(),
    ));
    let selector = MemoryLongTermSelector {
        query: LongTermMemoryQuery {
            kind: Some(LongTermMemoryKind::Preference),
            limit: 10,
            ..LongTermMemoryQuery::default()
        },
        evidence_ref: Some(second_ref),
    };

    let preview = apply_long_term_memory_control_mutation(
        &store,
        &control,
        LongTermMemoryControlMutationRequest {
            operation: MemoryLongTermMutation::ForgetByQuery {
                selector: selector.clone(),
                confirmation_token: None,
            },
            reason: "preview_forget_only_second_evidence".to_string(),
            dry_run: true,
            actor_subject_id: Some("subject-human".to_string()),
            memory_space_id: Some("space-user".to_string()),
            now_secs: NOW_SECS + 6,
        },
    )
    .expect("forget preview by evidence");

    assert_eq!(
        preview.target_report.resolved_record_ids,
        vec![second_id.clone()]
    );
    let token = preview
        .policy_decision
        .confirmation_token
        .clone()
        .expect("confirmation token");

    let applied = apply_long_term_memory_control_mutation(
        &store,
        &control,
        LongTermMemoryControlMutationRequest {
            operation: MemoryLongTermMutation::ForgetByQuery {
                selector,
                confirmation_token: Some(token),
            },
            reason: "forget_only_second_evidence".to_string(),
            dry_run: false,
            actor_subject_id: Some("subject-human".to_string()),
            memory_space_id: Some("space-user".to_string()),
            now_secs: NOW_SECS + 7,
        },
    )
    .expect("forget by evidence");

    assert!(applied.accepted);
    assert_eq!(applied.affected_records[0].record_id, second_id);
    assert!(store.get(&first_id).unwrap().is_some());
    assert!(store.get(&second_id).unwrap().is_none());
}

#[test]
fn change_scope_reports_subject_visibility_without_host_role_names() {
    let store = InMemoryLongTermStore::default();
    let control = InMemoryControlStore::default();
    let record_id = store.seed(draft(
        LongTermMemoryKind::Preference,
        "private-output-style",
        "The user prefers terse engineering updates in private work.",
    ));

    let report = apply_long_term_memory_control_mutation(
        &store,
        &control,
        LongTermMemoryControlMutationRequest {
            operation: MemoryLongTermMutation::ChangeScope {
                target: MemoryLongTermTarget::TranscriptDerivedRef(derived_ref(&record_id)),
                source_scope: LongTermMemorySourceScope::User,
                subject_visibility: MemorySubjectVisibilityPolicy::OnlySubjects(vec![
                    "subject-human".to_string(),
                    "agent:assistant-main".to_string(),
                ]),
            },
            reason: "user_limited_subject_visibility".to_string(),
            dry_run: false,
            actor_subject_id: Some("subject-human".to_string()),
            memory_space_id: Some("space-user".to_string()),
            now_secs: NOW_SECS + 6,
        },
    )
    .expect("change scope");

    assert!(report.accepted);
    assert_eq!(
        report.projection_impact.subject_visibility,
        MemorySubjectVisibilityPolicy::OnlySubjects(vec![
            "subject-human".to_string(),
            "agent:assistant-main".to_string()
        ])
    );
    let rendered = format!("{report:?}");
    assert!(!rendered.contains("CEO"));
    assert!(!rendered.contains("BOSS"));
    assert!(!rendered.contains("财务总监"));
}

#[test]
fn suppression_policy_blocks_future_candidate_writes_and_reports_policy_id() {
    let control = InMemoryControlStore::default();
    let selector = MemoryGovernanceSelector {
        memory_space_id: Some("space-user".to_string()),
        subject_id: Some("agent:assistant-main".to_string()),
        kind: Some(LongTermMemoryKind::Preference),
        topic_pattern: Some("temporary-*".to_string()),
        source_chat_id: None,
        source_scope: None,
    };

    let report = apply_long_term_memory_governance_policy_mutation(
        &control,
        MemoryGovernancePolicyMutation::Suppress {
            selector: selector.clone(),
            duration: MemoryGovernanceSuppressionDuration::UntilManualResume,
        },
        "user_said_do_not_remember_temporary_preferences".to_string(),
        false,
        NOW_SECS + 7,
    )
    .expect("policy report");

    assert!(report.accepted);
    assert!(report.policy_id.is_some());
    assert_eq!(report.affected_future_writes, "suppressed");

    let policies = control.list_long_term_governance_policies(10).unwrap();
    assert_eq!(policies.len(), 1);
    assert!(policies[0].matches_candidate(
        Some("space-user"),
        Some("agent:assistant-main"),
        &LongTermMemoryKind::Preference,
        "temporary-ui-choice",
        None,
        LongTermMemorySourceScope::User
    ));
    assert!(policies[0].matches_candidate(
        Some("space-user"),
        Some("agent:assistant-main"),
        &LongTermMemoryKind::Preference,
        "temporary_ui_choice",
        None,
        LongTermMemorySourceScope::User
    ));
}

#[test]
fn resume_and_remove_suppression_only_remove_matching_policy_kind() {
    let control = InMemoryControlStore::default();
    let selector = MemoryGovernanceSelector {
        memory_space_id: Some("space-user".to_string()),
        subject_id: Some("agent:assistant-main".to_string()),
        kind: Some(LongTermMemoryKind::Preference),
        topic_pattern: Some("temporary-*".to_string()),
        source_chat_id: None,
        source_scope: None,
    };

    apply_long_term_memory_governance_policy_mutation(
        &control,
        MemoryGovernancePolicyMutation::Pause {
            selector: selector.clone(),
            expires_at: None,
        },
        "pause_temporary_preference_memory".to_string(),
        false,
        NOW_SECS + 8,
    )
    .expect("pause policy");
    apply_long_term_memory_governance_policy_mutation(
        &control,
        MemoryGovernancePolicyMutation::Suppress {
            selector: selector.clone(),
            duration: MemoryGovernanceSuppressionDuration::UntilManualResume,
        },
        "suppress_temporary_preference_memory".to_string(),
        false,
        NOW_SECS + 9,
    )
    .expect("suppression policy");

    let resume = apply_long_term_memory_governance_policy_mutation(
        &control,
        MemoryGovernancePolicyMutation::Resume {
            selector: selector.clone(),
        },
        "resume_paused_temporary_preference_memory".to_string(),
        false,
        NOW_SECS + 10,
    )
    .expect("resume policy");

    assert_eq!(resume.operation, "policy.resume");
    assert_eq!(resume.policy_decision.reason, "matched_policy_count=1");
    let policies = control.list_long_term_governance_policies(10).unwrap();
    assert_eq!(policies.len(), 1);
    assert_eq!(policies[0].kind, "suppress");

    let remove_suppression = apply_long_term_memory_governance_policy_mutation(
        &control,
        MemoryGovernancePolicyMutation::RemoveSuppression { selector },
        "remove_suppression_temporary_preference_memory".to_string(),
        false,
        NOW_SECS + 11,
    )
    .expect("remove suppression policy");

    assert_eq!(remove_suppression.operation, "policy.remove_suppression");
    assert_eq!(
        remove_suppression.policy_decision.reason,
        "matched_policy_count=1"
    );
    assert!(control
        .list_long_term_governance_policies(10)
        .unwrap()
        .is_empty());
}
