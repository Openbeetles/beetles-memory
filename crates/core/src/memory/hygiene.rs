//! Background hygiene jobs for archive evidence and factual-memory upkeep.

use crate::error::Result;
use crate::platform::SkillStorage;
use crate::skills::{govern_runtime_skills, RuntimeSkillGovernanceOutcome};
use crate::util::{current_unix_secs, truncate_content_to_max};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

use super::{
    build_archive_reconcile_drafts, maintain_archive_search_backend, memory_capability_profile,
    plan_governed_shared_memory, plan_long_term_memory_owner_mutation, LongTermMemoryDraft,
    LongTermMemoryEntry, LongTermMemoryEntryPlan, LongTermMemoryOwnerMutation,
    LongTermMemoryReadStore, MemoryProfile, MemoryStore, SessionMessage, SessionStore,
    SessionSummaryStore, SharedMemoryWriteSource, TurnLedgerStore, MAX_SESSION_ENTRIES,
};

const DAILY_AGGREGATE_MARKER: &str = "<!-- beetle:hygiene:daily-aggregate -->";
const DAILY_PLACEHOLDER_MARKER: &str = "<!-- beetle:hygiene:daily-placeholder -->";
const TRANSCRIPT_AGING_PREFIX: &str = "transcript-aging-";
const TRANSCRIPT_AGING_MAX_CHATS: usize = 4;
const DAILY_AGGREGATE_MIN_AGE_DAYS: u64 = 7;

pub struct MemoryHygieneContext<'a> {
    pub session_store: &'a dyn SessionStore,
    pub session_summary_store: &'a dyn SessionSummaryStore,
    pub memory_store: &'a dyn MemoryStore,
    pub turn_ledger_store: &'a dyn TurnLedgerStore,
    pub long_term_memory_store: &'a dyn LongTermMemoryReadStore,
    pub skill_storage: &'a dyn SkillStorage,
}

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct MemoryHygieneOutcome {
    pub daily_notes_aggregated: usize,
    pub transcripts_rolled_up: usize,
    pub sessions_gc: usize,
    pub factual_metadata_updates: usize,
    pub factual_evidence_compacted: usize,
    pub archive_index_maintained: bool,
    pub runtime_skill_governance: RuntimeSkillGovernanceOutcome,
    pub daily_aggregate_targets: Vec<String>,
    pub transcript_rollup_chat_ids: Vec<String>,
    pub factual_reconcile_topics: Vec<String>,
    pub factual_compaction_topics: Vec<String>,
    pub planned_long_term_entries: Vec<LongTermMemoryEntry>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryHygieneInspection {
    pub profile: String,
    pub cadence: String,
    pub current_chat_id: String,
    pub daily_aggregate_candidates: Vec<String>,
    pub transcript_rollup_candidates: Vec<String>,
    pub factual_reconcile_candidates: Vec<String>,
    pub factual_compaction_candidates: Vec<String>,
    pub report_only_repair_candidates: Vec<String>,
    pub runtime_skill_records: usize,
    pub summary: String,
}

#[derive(Default)]
struct DailyAggregateReport {
    aggregate_targets: Vec<String>,
    archived_note_names: Vec<String>,
}

#[derive(Default)]
struct TranscriptRollupReport {
    chat_ids: Vec<String>,
}

pub fn run_memory_hygiene_jobs(
    ctx: MemoryHygieneContext<'_>,
    current_chat_id: &str,
    profile: MemoryProfile,
    now_secs: u64,
) -> MemoryHygieneOutcome {
    let effective_now_secs = if now_secs > 0 {
        now_secs
    } else {
        current_unix_secs()
    };
    let daily_report =
        aggregate_old_daily_notes(ctx.memory_store, effective_now_secs).unwrap_or_default();
    let transcript_report = rollup_aging_transcripts(
        ctx.session_store,
        ctx.session_summary_store,
        ctx.memory_store,
    )
    .unwrap_or_default();
    let mut outcome = MemoryHygieneOutcome {
        daily_notes_aggregated: daily_report.archived_note_names.len(),
        transcripts_rolled_up: transcript_report.chat_ids.len(),
        // Session file removal belongs to the governed write-back plane; hygiene
        // can report it, but must not perform storage remove from post-reply jobs.
        sessions_gc: 0,
        daily_aggregate_targets: daily_report.aggregate_targets,
        transcript_rollup_chat_ids: transcript_report.chat_ids,
        ..MemoryHygieneOutcome::default()
    };
    let factual_drafts = build_archive_reconcile_drafts(
        ctx.session_store,
        ctx.long_term_memory_store,
        ctx.memory_store,
        ctx.turn_ledger_store,
        current_chat_id,
        profile,
        6,
    );
    let mut reconciled_entries = Vec::new();
    if !factual_drafts.is_empty() {
        let factual_topics = factual_drafts
            .iter()
            .map(|draft| draft.topic.clone())
            .collect::<Vec<_>>();
        let plan = plan_governed_shared_memory(
            ctx.long_term_memory_store,
            &factual_drafts,
            effective_now_secs,
            SharedMemoryWriteSource::HygieneReconcile,
        )
        .unwrap_or_default();
        outcome.factual_metadata_updates = plan.outcome.changed;
        reconciled_entries = plan.accepted_entries;
        outcome
            .planned_long_term_entries
            .extend(reconciled_entries.iter().cloned());
        outcome.factual_reconcile_topics = factual_topics;
    }
    let (compacted_entries, compacted_topics) = compact_factual_evidence_metadata(
        ctx.long_term_memory_store,
        &factual_drafts,
        &reconciled_entries,
        effective_now_secs,
    )
    .unwrap_or_default();
    outcome.factual_evidence_compacted = compacted_entries.len();
    outcome.planned_long_term_entries.extend(compacted_entries);
    outcome.factual_compaction_topics = compacted_topics;
    outcome.archive_index_maintained =
        maintain_archive_search_backend(ctx.session_store, ctx.memory_store, ctx.turn_ledger_store)
            .unwrap_or(false);
    outcome.runtime_skill_governance =
        govern_runtime_skills(ctx.skill_storage, effective_now_secs).unwrap_or_default();
    outcome
}

pub fn run_memory_retention_compaction(
    ctx: MemoryHygieneContext<'_>,
    current_chat_id: &str,
    profile: MemoryProfile,
    now_secs: u64,
) -> MemoryHygieneOutcome {
    run_memory_hygiene_jobs(ctx, current_chat_id, profile, now_secs)
}

pub fn inspect_memory_hygiene(
    ctx: MemoryHygieneContext<'_>,
    current_chat_id: &str,
    profile: MemoryProfile,
    now_secs: u64,
) -> MemoryHygieneInspection {
    let effective_now_secs = if now_secs > 0 {
        now_secs
    } else {
        current_unix_secs()
    };
    let daily_aggregate_candidates =
        collect_daily_aggregate_groups(ctx.memory_store, effective_now_secs)
            .unwrap_or_default()
            .into_keys()
            .map(|month_key| format!("{month_key}-archive.md"))
            .collect::<Vec<_>>();
    let transcript_rollup_candidates =
        collect_transcript_rollup_candidates(ctx.session_store, ctx.session_summary_store)
            .unwrap_or_default();
    let reconcile_drafts = build_archive_reconcile_drafts(
        ctx.session_store,
        ctx.long_term_memory_store,
        ctx.memory_store,
        ctx.turn_ledger_store,
        current_chat_id,
        profile,
        6,
    );
    let factual_reconcile_candidates = reconcile_drafts
        .iter()
        .cloned()
        .map(|draft| draft.topic)
        .collect::<Vec<_>>();
    let factual_compaction_candidates =
        collect_factual_compaction_candidates(ctx.long_term_memory_store, &reconcile_drafts)
            .unwrap_or_default()
            .into_iter()
            .map(|draft| draft.topic)
            .collect::<Vec<_>>();
    let report_only_repair_candidates = collect_report_only_repair_candidates(
        ctx.session_store,
        ctx.session_summary_store,
        current_chat_id,
    );
    let runtime_skill_records = ctx
        .skill_storage
        .list_names()
        .map(|names| {
            names
                .into_iter()
                .filter(|name| crate::skills::is_runtime_skill_name(name))
                .count()
        })
        .unwrap_or(0);
    let cadence = match memory_capability_profile(profile).background_hygiene_level {
        super::MemoryHygieneLevel::Minimal => "minimal",
        super::MemoryHygieneLevel::Standard => "standard",
    };
    let profile_label = match profile {
        MemoryProfile::Embedded => "embedded",
        MemoryProfile::Standard => "standard",
    };
    let mut summary = String::new();
    let _ = write!(
        summary,
        "cadence={} | daily={} | transcript={} | factual_reconcile={} | factual_compaction={} | runtime_skills={}",
        cadence,
        daily_aggregate_candidates.len(),
        transcript_rollup_candidates.len(),
        factual_reconcile_candidates.len(),
        factual_compaction_candidates.len(),
        runtime_skill_records
    );
    if !report_only_repair_candidates.is_empty() {
        let _ = write!(
            summary,
            " | report_only_repair={}",
            report_only_repair_candidates.len()
        );
    }
    MemoryHygieneInspection {
        profile: profile_label.to_string(),
        cadence: cadence.to_string(),
        current_chat_id: current_chat_id.to_string(),
        daily_aggregate_candidates,
        transcript_rollup_candidates,
        factual_reconcile_candidates,
        factual_compaction_candidates,
        report_only_repair_candidates,
        runtime_skill_records,
        summary,
    }
}

pub fn render_memory_hygiene_inspection_markdown(inspection: &MemoryHygieneInspection) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Memory Hygiene Inspection");
    let _ = writeln!(out, "- current_chat_id: {}", inspection.current_chat_id);
    let _ = writeln!(out, "- profile: {}", inspection.profile);
    let _ = writeln!(out, "- cadence: {}", inspection.cadence);
    let _ = writeln!(out, "- summary: {}", inspection.summary);
    render_hygiene_list(
        &mut out,
        "Daily aggregate candidates",
        &inspection.daily_aggregate_candidates,
    );
    render_hygiene_list(
        &mut out,
        "Transcript rollup candidates",
        &inspection.transcript_rollup_candidates,
    );
    render_hygiene_list(
        &mut out,
        "Factual reconcile candidates",
        &inspection.factual_reconcile_candidates,
    );
    render_hygiene_list(
        &mut out,
        "Factual compaction candidates",
        &inspection.factual_compaction_candidates,
    );
    render_hygiene_list(
        &mut out,
        "Report-only repair candidates",
        &inspection.report_only_repair_candidates,
    );
    let _ = writeln!(
        out,
        "\n## Runtime Skills\n- records: {}",
        inspection.runtime_skill_records
    );
    out.trim_end().to_string()
}

fn compact_factual_evidence_metadata(
    store: &dyn LongTermMemoryReadStore,
    reconcile_drafts: &[LongTermMemoryDraft],
    reconciled_entries: &[LongTermMemoryEntry],
    now_secs: u64,
) -> Result<(Vec<LongTermMemoryEntry>, Vec<String>)> {
    let compacted = collect_factual_compaction_candidates(store, reconcile_drafts)?;
    if compacted.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut changed = Vec::new();
    let mut topics = Vec::with_capacity(compacted.len());
    for draft in compacted {
        let Some(id) = draft.stable_id() else {
            continue;
        };
        let mutation = LongTermMemoryOwnerMutation::CompactEvidenceMetadata {
            supporting_citations: draft.supporting_citations,
            evidence_count: draft.evidence_count.unwrap_or(0),
        };
        let existing = reconciled_entries
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
            .or(store.get(&id)?);
        let Some(existing) = existing else {
            continue;
        };
        match plan_long_term_memory_owner_mutation(&existing, &mutation, now_secs) {
            LongTermMemoryEntryPlan::Updated(entry) => {
                changed.push(entry);
                topics.push(draft.topic);
            }
            LongTermMemoryEntryPlan::Noop => {}
            LongTermMemoryEntryPlan::Created(_) => {
                return Err(crate::error::Error::config(
                    "factual_evidence_compaction",
                    "owner compaction unexpectedly created a record",
                ))
            }
            LongTermMemoryEntryPlan::Rejected(reason) => {
                return Err(crate::error::Error::config(
                    "factual_evidence_compaction",
                    format!("owner compaction rejected: {reason:?}"),
                ))
            }
        }
    }
    Ok((changed, topics))
}

fn collect_daily_aggregate_groups(
    store: &dyn MemoryStore,
    now_secs: u64,
) -> Result<std::collections::BTreeMap<String, Vec<String>>> {
    let mut monthly: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for name in store.list_daily_note_names(usize::MAX)? {
        if !name.ends_with(".md")
            || name.contains("archive")
            || name.starts_with(TRANSCRIPT_AGING_PREFIX)
            || super::parse_daily_note_observed_at(&name).is_none()
        {
            continue;
        }
        let observed_at = super::parse_daily_note_observed_at(&name).unwrap_or(0);
        if now_secs > 0
            && now_secs.saturating_sub(observed_at) < DAILY_AGGREGATE_MIN_AGE_DAYS * 86_400
        {
            continue;
        }
        let content = store.get_daily_note(&name)?;
        if content.contains(DAILY_PLACEHOLDER_MARKER) {
            continue;
        }
        let month_key = name.chars().take(7).collect::<String>();
        monthly.entry(month_key).or_default().push(name);
    }
    Ok(monthly)
}

fn aggregate_old_daily_notes(
    store: &dyn MemoryStore,
    now_secs: u64,
) -> Result<DailyAggregateReport> {
    let mut report = DailyAggregateReport::default();
    for (month_key, mut names) in collect_daily_aggregate_groups(store, now_secs)? {
        if names.len() < 2 {
            continue;
        }
        names.sort();
        let aggregate_name = format!("{month_key}-archive.md");
        let mut body = String::new();
        body.push_str(DAILY_AGGREGATE_MARKER);
        body.push_str("\n# Daily Aggregate ");
        body.push_str(&month_key);
        body.push_str("\n\n");
        for name in &names {
            let content = store.get_daily_note(name)?;
            let preview = truncate_content_to_max(content.trim(), 220);
            body.push_str("- ");
            body.push_str(name);
            body.push_str(": ");
            body.push_str(preview.as_ref());
            body.push('\n');
        }
        store.write_daily_note(&aggregate_name, body.trim_end())?;
        report.aggregate_targets.push(aggregate_name.clone());
        for name in names {
            let content = store.get_daily_note(&name)?;
            let preview = truncate_content_to_max(content.trim(), 160);
            let placeholder = format!(
                "{DAILY_PLACEHOLDER_MARKER}\nArchived into {aggregate_name}.\nSummary: {}",
                preview
            );
            store.write_daily_note(&name, &placeholder)?;
            report.archived_note_names.push(name);
        }
    }
    Ok(report)
}

fn rollup_aging_transcripts(
    session_store: &dyn SessionStore,
    session_summary_store: &dyn SessionSummaryStore,
    memory_store: &dyn MemoryStore,
) -> Result<TranscriptRollupReport> {
    let mut report = TranscriptRollupReport::default();
    for chat_id in session_store
        .list_chat_ids()?
        .into_iter()
        .take(TRANSCRIPT_AGING_MAX_CHATS)
    {
        let Some((summary, count)) = session_summary_store.get_with_count(&chat_id)? else {
            continue;
        };
        if summary.trim().is_empty() {
            continue;
        }
        let note_name = format!("{TRANSCRIPT_AGING_PREFIX}{}.md", short_chat_slug(&chat_id));
        let content = format!(
            "<!-- beetle:hygiene:transcript-rollup -->\nChat: {chat_id}\nMessages summarized: {count}\n\n{summary}"
        );
        memory_store.write_daily_note(&note_name, &content)?;
        report.chat_ids.push(chat_id);
    }
    Ok(report)
}

fn collect_transcript_rollup_candidates(
    session_store: &dyn SessionStore,
    session_summary_store: &dyn SessionSummaryStore,
) -> Result<Vec<String>> {
    let mut chat_ids = Vec::new();
    for chat_id in session_store
        .list_chat_ids()?
        .into_iter()
        .take(TRANSCRIPT_AGING_MAX_CHATS)
    {
        let Some((summary, _)) = session_summary_store.get_with_count(&chat_id)? else {
            continue;
        };
        if summary.trim().is_empty() {
            continue;
        }
        chat_ids.push(chat_id);
    }
    Ok(chat_ids)
}

fn collect_factual_compaction_candidates(
    store: &dyn LongTermMemoryReadStore,
    reconcile_drafts: &[LongTermMemoryDraft],
) -> Result<Vec<LongTermMemoryDraft>> {
    let mut compacted = Vec::new();
    let list_limit = store.count().unwrap_or(24).max(24);
    for entry in store.list(list_limit)? {
        let mut citations = entry.supporting_citations.clone();
        citations.sort();
        citations.dedup();
        citations.truncate(6);
        let mut should_compact = citations != entry.supporting_citations;
        let mut draft = LongTermMemoryDraft {
            kind: entry.kind.clone(),
            topic: entry.topic.clone(),
            content: entry.content.clone(),
            keywords: entry.keywords.clone(),
            privacy: entry.privacy,
            source_chat_id: entry.source_chat_id.clone(),
            source_type: Some(entry.source_type),
            source_scope: Some(entry.source_scope),
            subject_visibility: entry.subject_visibility.clone(),
            provenance: entry.provenance,
            confidence: Some(entry.confidence),
            freshness: Some(entry.freshness),
            stale_hint: Some(entry.stale_hint),
            supporting_citations: citations,
            canonical_entities: entry.canonical_entities.clone(),
            evidence_count: Some(
                entry
                    .evidence_count
                    .min(6)
                    .max(entry.supporting_citations.len().min(6) as u32),
            ),
            observed_at: Some(entry.observed_at),
            source_revision: entry.source_revision,
        };
        if draft.evidence_count != Some(entry.evidence_count) {
            should_compact = true;
        }
        if let Some(reconcile) = reconcile_drafts
            .iter()
            .find(|candidate| candidate.kind == entry.kind && candidate.topic == entry.topic)
        {
            if reconcile.evidence_count.unwrap_or(0) > draft.evidence_count.unwrap_or(0) {
                draft.evidence_count = reconcile.evidence_count;
                should_compact = true;
            }
        }
        if should_compact {
            compacted.push(draft);
        }
    }
    Ok(compacted)
}

fn collect_report_only_repair_candidates(
    session_store: &dyn SessionStore,
    summary_store: &dyn SessionSummaryStore,
    current_chat_id: &str,
) -> Vec<String> {
    let mut chat_ids = session_store.list_chat_ids().unwrap_or_default();
    if !chat_ids.iter().any(|chat_id| chat_id == current_chat_id) {
        chat_ids.push(current_chat_id.to_string());
    }
    chat_ids.sort();
    chat_ids.dedup();
    chat_ids.truncate(32);

    let mut candidates = Vec::new();
    for chat_id in chat_ids {
        if let Ok(messages) = session_store.load_recent(&chat_id, MAX_SESSION_ENTRIES) {
            collect_session_repair_candidates(&chat_id, &messages, &mut candidates);
        }
        if let Ok(Some(summary)) = summary_store.get(&chat_id) {
            if looks_like_assistant_identity_self_claim(&summary) {
                candidates.push(format!(
                    "chat={} kind=summary_assistant_identity_self_claim evidence={}",
                    chat_id,
                    truncate_repair_evidence(&summary)
                ));
            }
        }
    }
    candidates
}

fn collect_session_repair_candidates(
    chat_id: &str,
    messages: &[SessionMessage],
    candidates: &mut Vec<String>,
) {
    let mut prior_user_messages = Vec::<&str>::new();
    for (idx, message) in messages.iter().enumerate() {
        if message.role.eq_ignore_ascii_case("assistant")
            && looks_like_assistant_identity_self_claim(&message.content)
        {
            candidates.push(format!(
                "chat={} message={} kind=assistant_identity_self_claim evidence={}",
                chat_id,
                idx,
                truncate_repair_evidence(&message.content)
            ));
        }
        if message.role.eq_ignore_ascii_case("user") {
            if user_message_looks_like_full_history_duplicate(
                &message.content,
                &prior_user_messages,
            ) {
                candidates.push(format!(
                    "chat={} message={} kind=full_history_user_duplicate evidence={}",
                    chat_id,
                    idx,
                    truncate_repair_evidence(&message.content)
                ));
            }
            prior_user_messages.push(message.content.as_str());
        }
    }
}

fn user_message_looks_like_full_history_duplicate(
    content: &str,
    prior_user_messages: &[&str],
) -> bool {
    if prior_user_messages.is_empty() || !content.contains('\n') {
        return false;
    }
    let lines = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if lines.len() < 2 {
        return false;
    }
    prior_user_messages
        .iter()
        .map(|message| message.trim())
        .filter(|message| !message.is_empty())
        .any(|message| lines.contains(&message))
}

fn looks_like_assistant_identity_self_claim(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    lower.contains("indexedhybrid")
        || lower.contains("archive corpus")
        || lower.contains("memory helper")
        || lower.contains("beetle memory")
        || content.contains("记忆助手")
}

fn truncate_repair_evidence(content: &str) -> String {
    truncate_content_to_max(content.trim(), 120).into_owned()
}

fn render_hygiene_list(out: &mut String, title: &str, values: &[String]) {
    let _ = writeln!(out, "\n## {}", title);
    if values.is_empty() {
        let _ = writeln!(out, "- none");
        return;
    }
    for value in values {
        let _ = writeln!(out, "- {}", value);
    }
}

fn short_chat_slug(chat_id: &str) -> String {
    let slug = chat_id
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>();
    slug.chars().take(24).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::memory::{
        LongTermMemoryConfidence, LongTermMemoryDraft, LongTermMemoryEntry,
        LongTermMemoryFreshness, LongTermMemoryKind, LongTermMemorySlot, LongTermMemorySourceScope,
        LongTermMemorySourceType, LongTermMemoryStaleHint, LongTermMemoryStore, MemoryPrivacyClass,
        SessionMessage, TurnLedger,
    };
    use crate::platform::SkillStorage;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubMemoryStore {
        notes: Mutex<HashMap<String, String>>,
    }

    impl crate::memory::MemoryStore for StubMemoryStore {
        fn get_memory(&self) -> Result<String> {
            Ok(String::new())
        }
        fn set_memory(&self, _content: &str) -> Result<()> {
            Ok(())
        }
        fn list_daily_note_names(&self, recent_n: usize) -> Result<Vec<String>> {
            let mut names = self
                .notes
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            names.sort_by(|a, b| b.cmp(a));
            names.truncate(recent_n);
            Ok(names)
        }
        fn get_daily_note(&self, name: &str) -> Result<String> {
            Ok(self
                .notes
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(name)
                .cloned()
                .unwrap_or_default())
        }
        fn write_daily_note(&self, name: &str, content: &str) -> Result<()> {
            self.notes
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(name.to_string(), content.to_string());
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubSessionStore;

    impl crate::memory::SessionStore for StubSessionStore {
        fn append(&self, _chat_id: &str, _role: &str, _content: &str) -> Result<()> {
            Ok(())
        }
        fn load_recent(&self, _chat_id: &str, _n: usize) -> Result<Vec<SessionMessage>> {
            Ok(Vec::new())
        }
        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
        fn list_chat_ids(&self) -> Result<Vec<String>> {
            Ok(vec!["chat-1".to_string()])
        }
    }

    #[derive(Default)]
    struct StubSummaryStore;

    impl crate::memory::SessionSummaryStore for StubSummaryStore {
        fn get(&self, _chat_id: &str) -> Result<Option<String>> {
            Ok(Some("summary".to_string()))
        }
        fn set(&self, _chat_id: &str, _summary: &str) -> Result<()> {
            Ok(())
        }
        fn get_with_count(&self, _chat_id: &str) -> Result<Option<(String, usize)>> {
            Ok(Some(("summary".to_string(), 8)))
        }
    }

    #[derive(Default)]
    struct RepairSessionStore {
        messages: Mutex<HashMap<String, Vec<SessionMessage>>>,
    }

    impl crate::memory::SessionStore for RepairSessionStore {
        fn append(&self, chat_id: &str, role: &str, content: &str) -> Result<()> {
            self.messages
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .entry(chat_id.to_string())
                .or_default()
                .push(SessionMessage::synthetic(
                    role.to_string(),
                    content.to_string(),
                ));
            Ok(())
        }

        fn load_recent(&self, chat_id: &str, n: usize) -> Result<Vec<SessionMessage>> {
            let messages = self.messages.lock().unwrap_or_else(|e| e.into_inner());
            Ok(messages
                .get(chat_id)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .rev()
                .take(n)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect())
        }

        fn message_count(&self, chat_id: &str) -> Result<usize> {
            Ok(self
                .messages
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(chat_id)
                .map(Vec::len)
                .unwrap_or(0))
        }

        fn clear(&self, chat_id: &str) -> Result<()> {
            self.messages
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(chat_id);
            Ok(())
        }

        fn list_chat_ids(&self) -> Result<Vec<String>> {
            Ok(self
                .messages
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .keys()
                .cloned()
                .collect())
        }
    }

    #[derive(Default)]
    struct RepairSummaryStore {
        summaries: Mutex<HashMap<String, String>>,
    }

    impl crate::memory::SessionSummaryStore for RepairSummaryStore {
        fn get(&self, chat_id: &str) -> Result<Option<String>> {
            Ok(self
                .summaries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(chat_id)
                .cloned())
        }

        fn set(&self, chat_id: &str, summary: &str) -> Result<()> {
            self.summaries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(chat_id.to_string(), summary.to_string());
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubTurnLedgerStore;

    impl crate::memory::TurnLedgerStore for StubTurnLedgerStore {
        fn get(&self, _chat_id: &str) -> Result<Option<TurnLedger>> {
            Ok(None)
        }

        fn set(&self, _chat_id: &str, _ledger: &TurnLedger) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubSkillStorage {
        files: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl SkillStorage for StubSkillStorage {
        fn list_names(&self) -> Result<Vec<String>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .keys()
                .cloned()
                .collect())
        }

        fn read(&self, name: &str) -> Result<Vec<u8>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(name)
                .cloned()
                .unwrap_or_default())
        }

        fn write(&self, name: &str, content: &[u8]) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(name.to_string(), content.to_vec());
            Ok(())
        }

        fn remove(&self, name: &str) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(name);
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubLongTermMemoryStore {
        entries: Mutex<Vec<LongTermMemoryEntry>>,
        upserts: Mutex<Vec<Vec<LongTermMemoryDraft>>>,
    }

    impl crate::memory::LongTermMemoryStore for StubLongTermMemoryStore {
        fn upsert_many(&self, drafts: &[LongTermMemoryDraft], _now_secs: u64) -> Result<usize> {
            self.upserts
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(drafts.to_vec());
            Ok(drafts.len())
        }

        fn recall(
            &self,
            _query: &str,
            _source_chat_id: Option<&str>,
            _limit: usize,
        ) -> Result<Vec<LongTermMemoryEntry>> {
            Ok(Vec::new())
        }

        fn get(&self, id: &str) -> Result<Option<LongTermMemoryEntry>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .find(|entry| entry.id == id)
                .cloned())
        }

        fn mutate_owner(
            &self,
            id: &str,
            mutation: &LongTermMemoryOwnerMutation,
            now_secs: u64,
        ) -> Result<LongTermMemoryEntryPlan> {
            let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
            let existing = entries
                .iter()
                .find(|entry| entry.id == id)
                .cloned()
                .ok_or_else(|| {
                    crate::error::Error::config(
                        "long_term_owner_mutation",
                        "owner record not found",
                    )
                })?;
            let plan =
                crate::memory::plan_long_term_memory_owner_mutation(&existing, mutation, now_secs);
            if let LongTermMemoryEntryPlan::Updated(updated) = &plan {
                let entry = entries
                    .iter_mut()
                    .find(|entry| entry.id == id)
                    .expect("existing owner");
                *entry = updated.clone();
            }
            Ok(plan)
        }

        fn list(&self, limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
            let mut entries = self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            entries.truncate(limit);
            Ok(entries)
        }

        fn delete(&self, _id: &str) -> Result<bool> {
            Ok(false)
        }

        fn delete_slot(&self, _slot: &LongTermMemorySlot) -> Result<bool> {
            Ok(false)
        }

        fn count(&self) -> Result<usize> {
            Ok(self.entries.lock().unwrap_or_else(|e| e.into_inner()).len())
        }
    }

    #[test]
    fn aggregates_old_daily_notes_into_month_archive() {
        let store = StubMemoryStore::default();
        store.write_daily_note("2026-03-01.md", "第一天").unwrap();
        store.write_daily_note("2026-03-02.md", "第二天").unwrap();
        let report = aggregate_old_daily_notes(&store, 1_775_000_000).unwrap();
        assert_eq!(report.archived_note_names.len(), 2);
        assert_eq!(report.aggregate_targets, vec!["2026-03-archive.md"]);
        let aggregate = store.get_daily_note("2026-03-archive.md").unwrap();
        assert!(aggregate.contains("2026-03-01.md"));
        assert!(store
            .get_daily_note("2026-03-01.md")
            .unwrap()
            .contains("Archived into"));
    }

    #[test]
    fn transcript_rollup_writes_daily_note() {
        let session_store = StubSessionStore;
        let summary_store = StubSummaryStore;
        let memory_store = StubMemoryStore::default();
        let report =
            rollup_aging_transcripts(&session_store, &summary_store, &memory_store).unwrap();
        assert_eq!(report.chat_ids, vec!["chat-1".to_string()]);
        assert!(memory_store
            .get_daily_note("transcript-aging-chat-1.md")
            .unwrap()
            .contains("summary"));
    }

    #[test]
    fn factual_evidence_compaction_preserves_observed_at_and_deduplicates_citations() {
        let owner_id = LongTermMemorySlot {
            kind: LongTermMemoryKind::Fact,
            topic: "router_position".to_string(),
        }
        .stable_id()
        .expect("owner id");
        let store = StubLongTermMemoryStore {
            entries: Mutex::new(vec![LongTermMemoryEntry {
                id: owner_id.clone(),
                kind: LongTermMemoryKind::Fact,
                privacy: MemoryPrivacyClass::SharedWithSubject,
                topic: "router_position".to_string(),
                content: "Router sits near the window.".to_string(),
                keywords: vec!["router".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                source_type: LongTermMemorySourceType::Conversation,
                source_scope: LongTermMemorySourceScope::User,
                subject_visibility: crate::memory::MemorySubjectVisibilityPolicy::AllSubjects,
                provenance: crate::memory::LongTermMemoryProvenance {
                    source_authority: crate::memory::MemoryEvidenceAuthority::UserAsserted,
                    semantic_judgment_source: None,
                },
                confidence: LongTermMemoryConfidence::Medium,
                freshness: LongTermMemoryFreshness::Stable,
                stale_hint: LongTermMemoryStaleHint::None,
                supporting_citations: vec![
                    "transcript:chat-1#message=1".to_string(),
                    "transcript:chat-1#message=1".to_string(),
                    "daily_note:2026-04-02.md".to_string(),
                ],
                canonical_entities: Vec::new(),
                evidence_count: 1,
                created_at: 3,
                updated_at: 9,
                observed_at: 7,
                last_confirmed_at: Some(5),
                source_revision: None,
                owner_revision: 1,
                last_used_at: 0,
            }]),
            upserts: Mutex::new(Vec::new()),
        };

        let (changed, topics) = compact_factual_evidence_metadata(&store, &[], &[], 100).unwrap();
        assert_eq!(changed.len(), 1);
        assert_eq!(topics, vec!["router_position".to_string()]);
        let entry = &changed[0];
        assert_eq!(entry.observed_at, 7);
        assert_eq!(entry.last_confirmed_at, Some(5));
        assert_eq!(entry.supporting_citations.len(), 2);
        assert_eq!(entry.evidence_count, 3);
        assert_eq!(entry.owner_revision, 2);
        assert_eq!(
            LongTermMemoryStore::get(&store, &owner_id)
                .unwrap()
                .unwrap()
                .owner_revision,
            1
        );
    }

    #[test]
    fn hygiene_runs_runtime_skill_governance() {
        let session_store = StubSessionStore;
        let summary_store = StubSummaryStore;
        let memory_store = StubMemoryStore::default();
        let long_term_memory_store = StubLongTermMemoryStore::default();
        let turn_ledger_store = StubTurnLedgerStore;
        let skill_storage = StubSkillStorage::default();
        skill_storage
            .write(
                "runtime_skill__temp_probe",
                br#"<!-- beetle:runtime-skill -->
# Temp probe

Type: procedural_runtime_skill
Topic: temp_probe
Source chat: chat-1
Status: active
Observed at: 1
Updated at: 1
Use count: 0
Quality: 20

## Summary

## Procedure
probe"#,
            )
            .unwrap();

        let outcome = run_memory_hygiene_jobs(
            MemoryHygieneContext {
                session_store: &session_store,
                session_summary_store: &summary_store,
                memory_store: &memory_store,
                turn_ledger_store: &turn_ledger_store,
                long_term_memory_store: &long_term_memory_store,
                skill_storage: &skill_storage,
            },
            "chat-1",
            crate::memory::MemoryProfile::Embedded,
            90 * 86_400 + 10,
        );

        assert_eq!(outcome.runtime_skill_governance.pruned, 1);
        assert!(skill_storage
            .files
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty());
    }

    #[test]
    fn hygiene_inspection_reports_repair_candidates_without_mutating_store() {
        let session_store = RepairSessionStore::default();
        session_store
            .append("chat-1", "user", "call me Qingchuan")
            .unwrap();
        session_store
            .append("chat-1", "assistant", "我是 Beetle Memory 的记忆助手。")
            .unwrap();
        session_store
            .append("chat-1", "user", "call me Qingchuan\nI like cold brew")
            .unwrap();
        let summary_store = RepairSummaryStore::default();
        summary_store
            .set("chat-1", "assistant: 我是记忆助手，基于 IndexedHybrid")
            .unwrap();
        let memory_store = StubMemoryStore::default();
        let long_term_memory_store = StubLongTermMemoryStore::default();
        let turn_ledger_store = StubTurnLedgerStore;
        let skill_storage = StubSkillStorage::default();

        let inspection = inspect_memory_hygiene(
            MemoryHygieneContext {
                session_store: &session_store,
                session_summary_store: &summary_store,
                memory_store: &memory_store,
                turn_ledger_store: &turn_ledger_store,
                long_term_memory_store: &long_term_memory_store,
                skill_storage: &skill_storage,
            },
            "chat-1",
            crate::memory::MemoryProfile::Standard,
            90 * 86_400 + 10,
        );

        assert!(inspection
            .report_only_repair_candidates
            .iter()
            .any(|candidate| candidate.contains("assistant_identity_self_claim")));
        assert!(inspection
            .report_only_repair_candidates
            .iter()
            .any(|candidate| candidate.contains("full_history_user_duplicate")));
        assert!(inspection
            .report_only_repair_candidates
            .iter()
            .any(|candidate| candidate.contains("summary_assistant_identity_self_claim")));
        assert_eq!(session_store.message_count("chat-1").unwrap(), 3);
        assert!(render_memory_hygiene_inspection_markdown(&inspection)
            .contains("Report-only repair candidates"));
    }
}
