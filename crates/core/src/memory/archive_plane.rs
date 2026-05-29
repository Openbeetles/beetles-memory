//! Archive evidence plane: transcript / daily notes / turn logs as non-canonical sources.

use crate::memory::{MemoryStore, SessionStore, TurnLedgerStore};
use crate::util::truncate_content_to_max;

use super::{
    memory_capability_profile, render_turn_observation_ledger_block,
    search_archive_records_detailed, select_archive_hits_for_prompt_with_report,
    ArchiveSearchQuery, MemoryProfile, MAX_ARCHIVE_SEARCH_LIMIT,
};

const MAX_ARCHIVE_EVIDENCE_BLOCK_LEN: usize = 768;
const MIN_ARCHIVE_EVIDENCE_BLOCK_LEN: usize = 220;

fn archive_prompt_source_label(source: super::ArchiveRecordSource) -> &'static str {
    match source {
        super::ArchiveRecordSource::Transcript => "conversation record",
        super::ArchiveRecordSource::DailyNote => "daily memory note",
        super::ArchiveRecordSource::TurnLog => "turn record",
    }
}

fn archive_prompt_cues(hit: &super::ArchiveSearchHit) -> String {
    let cues = hit
        .cues
        .iter()
        .filter_map(|cue| {
            let cue = cue.trim();
            if cue.is_empty() || cue.starts_with("match:") || cue.starts_with("recent+") {
                None
            } else {
                Some(cue)
            }
        })
        .collect::<Vec<_>>();
    if cues.is_empty() {
        "supporting evidence".to_string()
    } else {
        cues.join(", ")
    }
}

fn render_archive_prompt_hit(hit: &super::ArchiveSearchHit) -> String {
    format!(
        "- [{}] {} ({})",
        archive_prompt_source_label(hit.source),
        hit.excerpt,
        archive_prompt_cues(hit)
    )
}

pub fn build_archive_evidence_block(
    session_store: &dyn SessionStore,
    memory_store: &dyn MemoryStore,
    turn_ledger_store: &dyn TurnLedgerStore,
    chat_id: &str,
    query: &str,
    system_max_len: usize,
    profile: MemoryProfile,
) -> Option<String> {
    let capability = memory_capability_profile(profile);
    let max_items = capability.archive_prompt_max_items;
    let cap = capability.archive_prompt_max_chars;
    let block_max_len = system_max_len.min(cap).min(MAX_ARCHIVE_EVIDENCE_BLOCK_LEN);
    if block_max_len < MIN_ARCHIVE_EVIDENCE_BLOCK_LEN {
        return None;
    }
    let search = search_archive_records_detailed(
        session_store,
        memory_store,
        turn_ledger_store,
        ArchiveSearchQuery {
            query,
            preferred_chat_id: Some(chat_id),
            chat_id_filter: None,
            sources: &[],
            limit: MAX_ARCHIVE_SEARCH_LIMIT,
        },
    )
    .ok()?;
    let selection = select_archive_hits_for_prompt_with_report(
        search.hits.clone(),
        profile,
        block_max_len.saturating_sub(128),
    );
    let selected = selection.hits;
    if selected.is_empty() {
        return build_archive_fallback_block(
            session_store,
            memory_store,
            turn_ledger_store,
            chat_id,
            block_max_len,
        );
    }

    let mut out = String::from(
        "## Archive evidence\nSupporting records only. These are evidence sources, not canonical shared memory. Use them to verify, cite, or distill factual memory updates.\n",
    );
    let mut appended = 0usize;
    for hit in selected.into_iter().take(max_items) {
        let line = render_archive_prompt_hit(&hit);
        if out.len().saturating_add(line.len()).saturating_add(1) > block_max_len {
            break;
        }
        out.push_str(&line);
        out.push('\n');
        appended += 1;
    }
    (appended > 0).then(|| out.trim_end().to_string())
}

fn build_archive_fallback_block(
    session_store: &dyn SessionStore,
    memory_store: &dyn MemoryStore,
    turn_ledger_store: &dyn TurnLedgerStore,
    chat_id: &str,
    block_max_len: usize,
) -> Option<String> {
    let mut out = String::from(
        "## Archive evidence\nSupporting records only. These are evidence sources, not canonical shared memory. Use them to verify, cite, or distill factual memory updates.\n",
    );
    let mut appended = 0usize;
    for message in session_store
        .load_recent(chat_id, 3)
        .unwrap_or_default()
        .into_iter()
        .rev()
    {
        let content = message.content.trim();
        if content.is_empty() {
            continue;
        }
        let line = format!(
            "- [{}] {} (fallback transcript)",
            message.role.to_uppercase(),
            truncate_content_to_max(content, 160)
        );
        if out.len().saturating_add(line.len()).saturating_add(1) > block_max_len {
            break;
        }
        out.push_str(&line);
        out.push('\n');
        appended += 1;
    }
    if appended == 0 {
        if let Some(name) = memory_store
            .list_daily_note_names(1)
            .unwrap_or_default()
            .into_iter()
            .next()
        {
            if let Ok(note) = memory_store.get_daily_note(&name) {
                let line = format!(
                    "- [{}] {} (fallback daily note)",
                    name,
                    truncate_content_to_max(note.trim(), 160)
                );
                if out.len().saturating_add(line.len()).saturating_add(1) <= block_max_len {
                    out.push_str(&line);
                    out.push('\n');
                    appended += 1;
                }
            }
        }
    }
    if appended == 0 {
        match turn_ledger_store.get(chat_id) {
            Ok(Some(ledger)) => {
                let mut preview = [
                    (!ledger.reason.trim().is_empty())
                        .then(|| format!("reason={}", ledger.reason.trim())),
                    (!ledger.user_preview.trim().is_empty())
                        .then(|| format!("user={}", ledger.user_preview.trim())),
                    (!ledger.reply_preview.trim().is_empty())
                        .then(|| format!("reply={}", ledger.reply_preview.trim())),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
                if let Some(observation_summary) =
                    ledger.observation.as_ref().and_then(|observation| {
                        render_turn_observation_ledger_block(observation, 220)
                            .map(|block| block.lines().skip(1).collect::<Vec<_>>().join(" | "))
                    })
                {
                    preview.push(format!("observation={observation_summary}"));
                }
                let preview = preview.join("; ");
                if !preview.is_empty() {
                    let line = format!(
                        "- [turn log] {} (fallback execution log)",
                        truncate_content_to_max(&preview, 160)
                    );
                    if out.len().saturating_add(line.len()).saturating_add(1) <= block_max_len {
                        out.push_str(&line);
                        out.push('\n');
                        appended += 1;
                    }
                }
            }
            Ok(None) => {}
            Err(error) => {
                let line = format!(
                    "- [turn log] unavailable (fallback degraded: {})",
                    error.stage()
                );
                if out.len().saturating_add(line.len()).saturating_add(1) <= block_max_len {
                    out.push_str(&line);
                    out.push('\n');
                    appended += 1;
                }
            }
        }
    }
    (appended > 0).then(|| out.trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::memory::{SessionMessage, TurnLedger};

    struct StubSessionStore {
        messages: Vec<SessionMessage>,
    }

    impl SessionStore for StubSessionStore {
        fn append(&self, _chat_id: &str, _role: &str, _content: &str) -> Result<()> {
            Ok(())
        }

        fn load_recent(&self, _chat_id: &str, n: usize) -> Result<Vec<SessionMessage>> {
            let start = self.messages.len().saturating_sub(n);
            Ok(self.messages[start..].to_vec())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }

        fn list_chat_ids(&self) -> Result<Vec<String>> {
            Ok(vec!["chat-1".to_string()])
        }
    }

    struct StubMemoryStore {
        note_name: String,
        note: String,
    }

    impl MemoryStore for StubMemoryStore {
        fn get_memory(&self) -> Result<String> {
            Ok(String::new())
        }

        fn set_memory(&self, _content: &str) -> Result<()> {
            Ok(())
        }

        fn list_daily_note_names(&self, _recent_n: usize) -> Result<Vec<String>> {
            Ok(vec![self.note_name.clone()])
        }

        fn get_daily_note(&self, _name: &str) -> Result<String> {
            Ok(self.note.clone())
        }

        fn write_daily_note(&self, _name: &str, _content: &str) -> Result<()> {
            Ok(())
        }
    }

    struct StubTurnLedgerStore;

    impl TurnLedgerStore for StubTurnLedgerStore {
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

    #[test]
    fn archive_evidence_prompt_hides_backend_trace_but_keeps_evidence() {
        let session_store = StubSessionStore {
            messages: vec![SessionMessage::synthetic(
                "user".to_string(),
                "release patch verification succeeded after checklist review".to_string(),
            )],
        };
        let memory_store = StubMemoryStore {
            note_name: "2026-05-23.md".to_string(),
            note: "Archive note: release patch verification succeeded after checklist review."
                .to_string(),
        };
        let block = build_archive_evidence_block(
            &session_store,
            &memory_store,
            &StubTurnLedgerStore,
            "chat-1",
            "release patch verification",
            2048,
            MemoryProfile::Standard,
        )
        .expect("archive evidence block");

        assert!(block.contains("## Archive evidence"));
        assert!(block.contains("release patch verification"));
        for forbidden in [
            "recall trace",
            "backend=",
            "IndexedHybrid",
            "selector=",
            "candidates=",
            "hits=",
            "why:",
            "hybrid fuzzy match",
            "primary quota pass",
            "match:",
            "recent+",
            "transcript:chat-1",
        ] {
            assert!(
                !block.contains(forbidden),
                "archive prompt leaked diagnostic term {forbidden}: {block}"
            );
        }
    }
}
