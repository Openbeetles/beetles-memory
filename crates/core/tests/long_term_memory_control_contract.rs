use std::collections::BTreeMap;
use std::sync::Mutex;

use bm_core::memory::{
    get_long_term_memory_control_detail, list_long_term_memory_control_page,
    plan_long_term_memory_control_mutation, plan_long_term_memory_governance_policy_mutation,
    plan_long_term_memory_owner_mutation, plan_long_term_memory_upsert, DerivedMemoryPlane,
    DerivedMemoryRef, LongTermControlOperation, LongTermMemoryControlAuditEvent,
    LongTermMemoryControlDetailRequest, LongTermMemoryControlListRequest,
    LongTermMemoryControlMutationRequest, LongTermMemoryControlReadStore,
    LongTermMemoryControlWrite, LongTermMemoryDraft, LongTermMemoryEntry, LongTermMemoryEntryPlan,
    LongTermMemoryKind, LongTermMemoryOwnerMutation, LongTermMemoryOwnerWrite, LongTermMemoryQuery,
    LongTermMemorySlot, LongTermMemorySourceScope, LongTermMemoryStaleHint, LongTermMemoryStore,
    MemoryGovernancePolicyMutation, MemoryGovernancePolicyMutationReport, MemoryGovernanceSelector,
    MemoryGovernanceSuppressionDuration, MemoryLongTermControlView, MemoryLongTermGovernancePolicy,
    MemoryLongTermMutation, MemoryLongTermSelector, MemoryLongTermTarget, MemoryPrivacyClass,
    MemorySubjectVisibilityPolicy, TranscriptEvidenceRef,
};
use bm_core::Result;

const NOW_SECS: u64 = 1_780_000_000;

#[test]
fn production_memory_api_does_not_export_writable_control_capabilities() {
    let memory_module = include_str!("../src/memory/mod.rs");
    let control_module = include_str!("../src/memory/long_term_control.rs");
    let control_exports = memory_module
        .split("pub use long_term_control::{")
        .nth(1)
        .and_then(|exports| exports.split("};").next())
        .expect("long-term control export block");

    assert!(!control_exports.contains("apply_long_term_memory_control_mutation"));
    assert!(!control_exports.contains("apply_long_term_memory_governance_policy_mutation"));
    assert!(!control_exports.contains("LongTermMemoryControlStore"));
    assert!(!control_module.contains("pub fn apply_long_term_memory_control_mutation"));
    assert!(!control_module.contains("pub fn apply_long_term_memory_governance_policy_mutation"));
    assert!(control_module.contains("pub(crate) fn apply_long_term_memory_control_mutation"));
    assert!(
        control_module.contains("pub(crate) fn apply_long_term_memory_governance_policy_mutation")
    );
}

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
            let id = draft.stable_id().expect("stable id");
            match plan_long_term_memory_upsert(entries.get(&id), draft, now_secs) {
                LongTermMemoryEntryPlan::Created(entry)
                | LongTermMemoryEntryPlan::Updated(entry) => {
                    entries.insert(id, entry);
                    changed += 1;
                }
                LongTermMemoryEntryPlan::Noop => {}
                LongTermMemoryEntryPlan::Rejected(reason) => panic!("rejected draft: {reason:?}"),
            }
        }
        Ok(changed)
    }

    fn mutate_owner(
        &self,
        id: &str,
        mutation: &LongTermMemoryOwnerMutation,
        now_secs: u64,
    ) -> Result<LongTermMemoryEntryPlan> {
        let mut entries = self.entries.lock().expect("entries lock");
        let existing = entries.get(id).expect("owner record").clone();
        let plan = plan_long_term_memory_owner_mutation(&existing, mutation, now_secs);
        if let LongTermMemoryEntryPlan::Updated(entry) = &plan {
            entries.insert(id.to_string(), entry.clone());
        }
        Ok(plan)
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

struct ReadOnlyLongTermView<'a>(&'a InMemoryLongTermStore);

impl bm_core::memory::LongTermMemoryReadStore for ReadOnlyLongTermView<'_> {
    fn recall(
        &self,
        query: &str,
        source_chat_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryEntry>> {
        LongTermMemoryStore::recall(self.0, query, source_chat_id, limit)
    }

    fn get(&self, id: &str) -> Result<Option<LongTermMemoryEntry>> {
        LongTermMemoryStore::get(self.0, id)
    }

    fn list(&self, limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
        LongTermMemoryStore::list(self.0, limit)
    }

    fn count(&self) -> Result<usize> {
        LongTermMemoryStore::count(self.0)
    }
}

#[derive(Default)]
struct InMemoryControlStore {
    revision_intents: Mutex<Vec<bm_core::memory::LongTermMemoryControlRevisionIntent>>,
    tombstones: Mutex<BTreeMap<String, bm_core::memory::LongTermMemoryTombstone>>,
    policies: Mutex<BTreeMap<String, MemoryLongTermGovernancePolicy>>,
    audits: Mutex<Vec<LongTermMemoryControlAuditEvent>>,
}

impl LongTermMemoryControlReadStore for InMemoryControlStore {
    fn list_long_term_control_revisions(
        &self,
        record_id: &str,
        limit: usize,
    ) -> Result<Vec<bm_core::memory::LongTermMemoryControlRevision>> {
        let _ = (record_id, limit);
        Ok(Vec::new())
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

impl InMemoryControlStore {
    fn put_long_term_control_revision_intent(
        &self,
        revision: &bm_core::memory::LongTermMemoryControlRevisionIntent,
    ) -> Result<()> {
        self.revision_intents
            .lock()
            .expect("revision intents lock")
            .push(revision.clone());
        Ok(())
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

    fn put_long_term_control_audit(&self, event: &LongTermMemoryControlAuditEvent) -> Result<()> {
        self.audits.lock().expect("audits lock").push(event.clone());
        Ok(())
    }
}

fn apply_long_term_memory_control_mutation(
    store: &InMemoryLongTermStore,
    control: &InMemoryControlStore,
    request: LongTermMemoryControlMutationRequest,
) -> Result<bm_core::memory::MemoryLongTermMutationReport> {
    let plan =
        plan_long_term_memory_control_mutation(store, &ReadOnlyControlView(control), request)?;
    for write in plan.owner_writes {
        match write {
            LongTermMemoryOwnerWrite::Put(entry) => {
                let entry = *entry;
                store
                    .entries
                    .lock()
                    .expect("entries lock")
                    .insert(entry.id.clone(), entry);
            }
            LongTermMemoryOwnerWrite::Delete { record_id } => {
                store
                    .entries
                    .lock()
                    .expect("entries lock")
                    .remove(&record_id);
            }
        }
    }
    apply_control_writes(control, plan.control_writes)?;
    Ok(plan.report)
}

fn apply_long_term_memory_governance_policy_mutation(
    control: &InMemoryControlStore,
    operation: MemoryGovernancePolicyMutation,
    reason: String,
    dry_run: bool,
    now_secs: u64,
) -> Result<MemoryGovernancePolicyMutationReport> {
    let plan = plan_long_term_memory_governance_policy_mutation(
        &ReadOnlyControlView(control),
        operation,
        reason,
        dry_run,
        now_secs,
    )?;
    apply_control_writes(control, plan.control_writes)?;
    Ok(plan.report)
}

fn apply_control_writes(
    control: &InMemoryControlStore,
    writes: Vec<LongTermMemoryControlWrite>,
) -> Result<()> {
    for write in writes {
        match write {
            LongTermMemoryControlWrite::PutRevisionIntent(revision) => {
                control.put_long_term_control_revision_intent(&revision)?;
            }
            LongTermMemoryControlWrite::PutTombstone(tombstone) => {
                control.put_long_term_control_tombstone(&tombstone)?;
            }
            LongTermMemoryControlWrite::PutGovernancePolicy(policy) => {
                control.put_long_term_governance_policy(&policy)?;
            }
            LongTermMemoryControlWrite::DeleteGovernancePolicy { policy_id, .. } => {
                control.delete_long_term_governance_policy(&policy_id)?;
            }
            LongTermMemoryControlWrite::AppendAudit(event) => {
                control.put_long_term_control_audit(&event)?;
            }
        }
    }
    Ok(())
}

struct ReadOnlyControlView<'a>(&'a InMemoryControlStore);

impl LongTermMemoryControlReadStore for ReadOnlyControlView<'_> {
    fn list_long_term_control_revisions(
        &self,
        record_id: &str,
        limit: usize,
    ) -> Result<Vec<bm_core::memory::LongTermMemoryControlRevision>> {
        self.0.list_long_term_control_revisions(record_id, limit)
    }

    fn get_long_term_control_tombstone(
        &self,
        record_id: &str,
    ) -> Result<Option<bm_core::memory::LongTermMemoryTombstone>> {
        self.0.get_long_term_control_tombstone(record_id)
    }

    fn list_long_term_control_tombstones(
        &self,
        limit: usize,
    ) -> Result<Vec<bm_core::memory::LongTermMemoryTombstone>> {
        self.0.list_long_term_control_tombstones(limit)
    }

    fn list_long_term_governance_policies(
        &self,
        limit: usize,
    ) -> Result<Vec<MemoryLongTermGovernancePolicy>> {
        self.0.list_long_term_governance_policies(limit)
    }

    fn list_long_term_control_audit(
        &self,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryControlAuditEvent>> {
        self.0.list_long_term_control_audit(limit)
    }
}

fn draft(kind: LongTermMemoryKind, topic: &str, content: &str) -> LongTermMemoryDraft {
    LongTermMemoryDraft {
        kind,
        privacy: MemoryPrivacyClass::SharedWithSubject,
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
        canonical_entities: Vec::new(),
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
fn control_planner_returns_write_intent_without_mutating_read_stores() {
    let store = InMemoryLongTermStore::default();
    let control = InMemoryControlStore::default();
    let record_id = store.seed(draft(
        LongTermMemoryKind::Preference,
        "preferred-editor",
        "The user prefers Helix for quick terminal edits.",
    ));
    let before = store.get(&record_id).expect("read before").expect("record");

    let plan = plan_long_term_memory_control_mutation(
        &ReadOnlyLongTermView(&store),
        &ReadOnlyControlView(&control),
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
            factual_owner_id: "space-user".to_string(),
            actor_subject_id: Some("subject-human".to_string()),
            memory_space_id: Some("space-user".to_string()),
            now_secs: NOW_SECS + 1,
        },
    )
    .expect("control plan");

    assert!(plan.report.accepted);
    assert_eq!(plan.owner_writes.len(), 1);
    assert_eq!(plan.control_writes.len(), 2);
    assert_eq!(store.get(&record_id).unwrap(), Some(before));
    assert!(control.revision_intents.lock().unwrap().is_empty());
    assert!(control.tombstones.lock().unwrap().is_empty());
    assert!(control.policies.lock().unwrap().is_empty());
    assert!(control.audits.lock().unwrap().is_empty());
}

#[test]
fn control_planner_rejects_subject_scoped_factual_owner() {
    let store = InMemoryLongTermStore::default();
    let control = InMemoryControlStore::default();
    let error = plan_long_term_memory_control_mutation(
        &ReadOnlyLongTermView(&store),
        &ReadOnlyControlView(&control),
        LongTermMemoryControlMutationRequest {
            operation: MemoryLongTermMutation::Delete {
                target: MemoryLongTermTarget::RecordId("missing-record".to_string()),
            },
            reason: "invalid_subject_owner".to_string(),
            dry_run: false,
            factual_owner_id: "agent:alpha".to_string(),
            actor_subject_id: Some("agent:alpha".to_string()),
            memory_space_id: Some("space-user".to_string()),
            now_secs: NOW_SECS + 1,
        },
    )
    .expect_err("subject-scoped factual owner must fail closed");

    assert!(error.to_string().contains("factual owner"));
}

#[test]
fn governance_planner_returns_write_intent_without_mutating_read_store() {
    let control = InMemoryControlStore::default();
    let plan = plan_long_term_memory_governance_policy_mutation(
        &ReadOnlyControlView(&control),
        MemoryGovernancePolicyMutation::Pause {
            selector: MemoryGovernanceSelector {
                memory_space_id: Some("space-user".to_string()),
                subject_id: Some("agent:assistant-main".to_string()),
                kind: Some(LongTermMemoryKind::Preference),
                topic_pattern: Some("temporary-*".to_string()),
                source_chat_id: None,
                source_scope: None,
            },
            expires_at: None,
        },
        "pause_temporary_preference_memory".to_string(),
        false,
        NOW_SECS + 2,
    )
    .expect("governance plan");

    assert!(plan.report.accepted);
    assert_eq!(plan.control_writes.len(), 2);
    assert!(control.policies.lock().unwrap().is_empty());
    assert!(control.audits.lock().unwrap().is_empty());
}

#[test]
fn list_and_detail_return_active_records_with_evidence_and_cursor() {
    let store = InMemoryLongTermStore::default();
    let control = InMemoryControlStore::default();
    let read_control = ReadOnlyControlView(&control);
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
        &read_control,
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
        &read_control,
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
        &read_control,
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
fn correct_preserves_source_lineage_and_increments_owner_revision() {
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
            factual_owner_id: "space-user".to_string(),
            actor_subject_id: Some("subject-human".to_string()),
            memory_space_id: Some("space-user".to_string()),
            now_secs: NOW_SECS + 1,
        },
    )
    .expect("correct report");

    assert!(report.accepted);
    assert_eq!(report.affected_records.len(), 1);
    assert_eq!(report.affected_records[0].record_id, record_id);
    assert_eq!(report.affected_records[0].previous_owner_revision, 1);
    assert_eq!(report.affected_records[0].new_owner_revision, Some(2));
    assert_eq!(report.affected_records[0].previous_source_revision, Some(1));
    assert_eq!(report.affected_records[0].new_source_revision, Some(1));
    let corrected = store.get(&record_id).expect("read").expect("corrected");
    assert_eq!(corrected.source_revision, Some(1));
    assert_eq!(corrected.owner_revision, 2);
    assert!(corrected.content.contains("Neovim"));
    assert_eq!(control.revision_intents.lock().unwrap().len(), 1);
    assert!(report.audit_event_id.is_some());
}

#[test]
fn unchanged_correct_stale_privacy_and_scope_are_noop_without_control_writes() {
    for operation_name in ["correct", "mark_stale", "change_privacy", "change_scope"] {
        let store = InMemoryLongTermStore::default();
        let control = InMemoryControlStore::default();
        let original = draft(
            LongTermMemoryKind::Preference,
            "preferred-editor",
            "The user prefers Helix for quick terminal edits.",
        );
        let record_id = store.seed(original.clone());
        let operation = match operation_name {
            "correct" => MemoryLongTermMutation::Correct {
                target: MemoryLongTermTarget::RecordId(record_id.clone()),
                replacement: original,
            },
            "mark_stale" => MemoryLongTermMutation::MarkStale {
                target: MemoryLongTermTarget::RecordId(record_id.clone()),
                stale_hint: LongTermMemoryStaleHint::None,
            },
            "change_privacy" => MemoryLongTermMutation::ChangePrivacy {
                target: MemoryLongTermTarget::RecordId(record_id.clone()),
                privacy: MemoryPrivacyClass::SharedWithSubject,
            },
            "change_scope" => MemoryLongTermMutation::ChangeScope {
                target: MemoryLongTermTarget::RecordId(record_id.clone()),
                source_scope: LongTermMemorySourceScope::User,
                subject_visibility: MemorySubjectVisibilityPolicy::AllSubjects,
            },
            _ => unreachable!(),
        };

        let report = apply_long_term_memory_control_mutation(
            &store,
            &control,
            LongTermMemoryControlMutationRequest {
                operation,
                reason: "same value".to_string(),
                dry_run: false,
                factual_owner_id: "space-user".to_string(),
                actor_subject_id: Some("subject-human".to_string()),
                memory_space_id: Some("space-user".to_string()),
                now_secs: NOW_SECS + 1,
            },
        )
        .expect("noop report");

        assert!(report.accepted, "{operation_name}");
        assert!(report.affected_records.is_empty(), "{operation_name}");
        assert_eq!(
            store.get(&record_id).unwrap().unwrap().owner_revision,
            1,
            "{operation_name}"
        );
        assert!(
            control.revision_intents.lock().unwrap().is_empty(),
            "{operation_name}"
        );
        assert!(
            control.audits.lock().unwrap().is_empty(),
            "{operation_name}"
        );
    }
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
            factual_owner_id: "space-user".to_string(),
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
            factual_owner_id: "space-user".to_string(),
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
            target: MemoryLongTermTarget::RecordId(new_id.clone()),
            view: MemoryLongTermControlView::HostUi,
        },
    )
    .expect("deleted detail");
    assert!(deleted_detail.record.is_none());
    assert!(deleted_detail.revisions.is_empty());
    let tombstone = deleted_detail.tombstone.expect("shared owner tombstone");
    assert_eq!(tombstone.record_id, new_id);
    assert_eq!(tombstone.memory_space_id, "space-user");
    assert_eq!(tombstone.factual_owner_id, "space-user");
    assert_eq!(tombstone.actor_subject_id.as_deref(), Some("subject-human"));
    assert!(deleted_detail.transcript_refs.is_empty());
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
            factual_owner_id: "space-user".to_string(),
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
            factual_owner_id: "space-user".to_string(),
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
            factual_owner_id: "space-user".to_string(),
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
            factual_owner_id: "space-user".to_string(),
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
            factual_owner_id: "space-user".to_string(),
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
fn privacy_transition_requires_the_explicit_control_operation() {
    let store = InMemoryLongTermStore::default();
    let control = InMemoryControlStore::default();
    let original = draft(
        LongTermMemoryKind::Preference,
        "privacy-transition",
        "Keep this memory subject-scoped until explicitly governed.",
    );
    let record_id = store.seed(original.clone());
    let mut implicit = original;
    implicit.privacy = MemoryPrivacyClass::PublicRuntime;

    let rejected = apply_long_term_memory_control_mutation(
        &store,
        &control,
        LongTermMemoryControlMutationRequest {
            operation: MemoryLongTermMutation::Correct {
                target: MemoryLongTermTarget::RecordId(record_id.clone()),
                replacement: implicit,
            },
            reason: "ordinary correction must not broaden visibility".to_string(),
            dry_run: false,
            factual_owner_id: "space-user".to_string(),
            actor_subject_id: Some("subject-human".to_string()),
            memory_space_id: Some("space-user".to_string()),
            now_secs: NOW_SECS + 8,
        },
    )
    .expect("rejected privacy correction");
    assert!(!rejected.accepted);
    assert_eq!(
        rejected.policy_decision.reason,
        "privacy_transition_requires_change_privacy"
    );
    assert_eq!(
        store.get(&record_id).unwrap().unwrap().privacy,
        MemoryPrivacyClass::SharedWithSubject
    );

    let changed = apply_long_term_memory_control_mutation(
        &store,
        &control,
        LongTermMemoryControlMutationRequest {
            operation: MemoryLongTermMutation::ChangePrivacy {
                target: MemoryLongTermTarget::RecordId(record_id.clone()),
                privacy: MemoryPrivacyClass::PublicRuntime,
            },
            reason: "owner explicitly approved public runtime visibility".to_string(),
            dry_run: false,
            factual_owner_id: "space-user".to_string(),
            actor_subject_id: Some("subject-human".to_string()),
            memory_space_id: Some("space-user".to_string()),
            now_secs: NOW_SECS + 9,
        },
    )
    .expect("explicit privacy transition");
    assert!(changed.accepted);
    assert_eq!(changed.operation, "change_privacy");
    assert_eq!(
        store.get(&record_id).unwrap().unwrap().privacy,
        MemoryPrivacyClass::PublicRuntime
    );
    let revision = control
        .revision_intents
        .lock()
        .unwrap()
        .iter()
        .find(|revision| revision.operation == LongTermControlOperation::ChangePrivacy)
        .cloned()
        .expect("change privacy revision");
    assert_eq!(
        revision.transition.predecessor.owner_revision, 1,
        "intent must bind the exact prior owner revision"
    );
    assert_eq!(
        revision
            .transition
            .successor
            .as_ref()
            .expect("successor")
            .owner_revision,
        2
    );
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
